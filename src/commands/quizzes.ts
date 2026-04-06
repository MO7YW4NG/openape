import { formatTimestamp, getOutputFormat } from "../lib/utils.ts";
import { Command } from "commander";
import type { OutputFormat } from "../lib/types.ts";
import {
  getEnrolledCoursesApi,
  getQuizzesByCoursesApi,
  startQuizAttemptApi,
  getQuizAttemptDataApi,
  getAllQuizAttemptDataApi,
  processQuizAttemptApi
} from "../lib/moodle.ts";
import { createApiContext } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";

function stripHtmlKeepLines(html: string): string {
  return html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/p>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function parseQuestionHtml(html: string): { text: string; options: string[] } {
  const qtextMatch = html.match(/<div class="qtext">([\s\S]*?)<\/div>\s*<\/div>/);
  const text = stripHtmlKeepLines(qtextMatch?.[1] ?? "");

  const options: string[] = [];
  const optionRegex = /data-region="answer-label">([\s\S]*?)<\/div>\s*<\/div>/g;
  let match;
  while ((match = optionRegex.exec(html)) !== null) {
    options.push(stripHtmlKeepLines(match[1]));
  }

  return { text, options };
}

function parseSavedAnswer(html: string): string | string[] | null {
  const radioChecked = html.match(/<input type="radio"[^>]*value="(\d+)"[^>]*checked="checked"/);
  if (radioChecked && radioChecked[1] !== "-1") return radioChecked[1];

  const checkboxChecked = [...html.matchAll(/<input type="checkbox"[^>]*name="[^"]*choice(\d+)"[^>]*checked="checked"/g)];
  if (checkboxChecked.length > 0) return checkboxChecked.map(m => m[1]);

  // Match <input> with both name="*_answer" and type="text" in any attribute order
  const textMatch = html.match(/<input[^>]*(?:name="[^"]*:_answer"|type="text")[^>]*(?:name="[^"]*:_answer"|type="text")[^>]*value="([^"]*)"/);
  if (textMatch && textMatch[1] !== "") return textMatch[1];

  return null;
}

function parseQuizQuestions(questions: Record<number, any>) {
  return Object.values(questions).map((q: any) => {
    const parsed = parseQuestionHtml(q.html ?? "");
    const savedAnswer = parseSavedAnswer(q.html ?? "");
    return {
      slot: q.slot,
      type: q.type,
      status: q.status,
      stateclass: q.stateclass,
      savedAnswer,
      question: parsed.text,
      options: parsed.options,
    };
  });
}

export function registerQuizzesCommand(program: Command): void {
  const quizzesCmd = program.command("quizzes");
  quizzesCmd.description("Quiz operations");

  quizzesCmd
    .command("list")
    .description("List incomplete quizzes in a course")
    .argument("<course-id>", "Course ID")
    .option("--all", "Include completed quizzes")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const quizzes = await getQuizzesByCoursesApi(apiContext.session, [parseInt(courseId, 10)]);

      // Default: only show incomplete quizzes
      const filtered = options.all ? quizzes : quizzes.filter(q => !q.isComplete);

      const formattedQuizzes = filtered.map(({ courseId, ...q }) => ({
        ...q,
        timeClose: q.timeClose ? formatTimestamp(q.timeClose) : null,
      }));

      formatAndOutput(formattedQuizzes as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  quizzesCmd
    .command("list-all")
    .description("List all incomplete quizzes across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .option("--all", "Include completed quizzes")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const classification = options.level === "all" ? undefined : "inprogress";
      const courses = await getEnrolledCoursesApi(apiContext.session, {
        classification,
      });

      // Get quizzes via WS API (no browser needed!)
      const courseIds = courses.map(c => c.id);
      const apiQuizzes = await getQuizzesByCoursesApi(apiContext.session, courseIds);

      // Build a map of courseId -> course for quick lookup
      const courseMap = new Map(courses.map(c => [c.id, c]));

      const allQuizzes: Array<{ courseName: string; courseId: number; name: string; url: string; quizid: string; isComplete: boolean; attemptsUsed: number; maxAttempts: number; timeClose: string | null }> = [];
      for (const q of apiQuizzes) {
        const course = courseMap.get(q.courseId);
        if (course && (options.all || !q.isComplete)) {
          allQuizzes.push({
            courseName: course.fullname,
            courseId: q.courseId,
            name: q.name,
            url: q.url,
            quizid: q.quizid,
            isComplete: q.isComplete,
            attemptsUsed: q.attemptsUsed,
            maxAttempts: q.maxAttempts,
            timeClose: q.timeClose ? formatTimestamp(q.timeClose) : null,
          });
        }
      }

      apiContext.log.info(`\n總計發現 ${allQuizzes.length} 個測驗。`);
      formatAndOutput(allQuizzes as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  quizzesCmd
    .command("start")
    .description("Start a new quiz attempt")
    .argument("<quiz-id>", "Quiz ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (quizCmid, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      try {
        const result = await startQuizAttemptApi(
          apiContext.session,
          quizCmid,
        );

        apiContext.log.success(`Quiz attempt ${result.attempt.attemptid} started.`);

        const attemptId = result.attempt.attemptid;
        const data = await getAllQuizAttemptDataApi(apiContext.session, attemptId);

        const questions = parseQuizQuestions(data.questions);

        const outputData = [{
          attemptId,
          quizId: result.attempt.quizid,
          state: result.attempt.state,
          timeStart: formatTimestamp(result.attempt.timestart),
          timeFinish: result.attempt.timefinish
            ? formatTimestamp(result.attempt.timefinish)
            : null,
          isPreview: result.attempt.preview,
          totalQuestions: questions.length,
          questions,
        }];

        formatAndOutput(outputData as unknown as Record<string, unknown>[], output, apiContext.log);
      } catch (error) {
        apiContext.log.error(`Failed to start quiz attempt: ${error instanceof Error ? error.message : String(error)}`);
        process.exitCode = 1;
      }
    });

  quizzesCmd
    .command("info")
    .description("Get quiz attempt data and questions")
    .argument("<attempt-id>", "Quiz attempt ID")
    .option("--page <number>", "Page number (-1 for all pages)", "-1")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (attemptId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      try {
        const pageNumber = parseInt(options.page);
        const data = pageNumber === -1
          ? await getAllQuizAttemptDataApi(apiContext.session, parseInt(attemptId))
          : await getQuizAttemptDataApi(apiContext.session, parseInt(attemptId), pageNumber);

        const questions = parseQuizQuestions(data.questions);

        const outputData = [{
          attemptId: data.attempt.attemptid,
          quizId: data.attempt.quizid,
          state: data.attempt.state,
          totalQuestions: questions.length,
          questions,
        }];

        apiContext.log.success(`Retrieved attempt ${data.attempt.attemptid}`);
        formatAndOutput(outputData as unknown as Record<string, unknown>[], output, apiContext.log);
      } catch (error) {
        apiContext.log.error(`Failed to get attempt data: ${error instanceof Error ? error.message : String(error)}`);
        process.exitCode = 1;
      }
    });

  quizzesCmd
    .command("save")
    .description("Save answers for a quiz attempt")
    .argument("<attempt-id>", "Quiz attempt ID")
    .argument("<answers>", "Answers JSON: [{slot:1,answer:\"0\"}]  multichoice=number, multichoices=\"0,2\", shortanswer=\"text\"")
    .option("--submit", "Submit the attempt after saving")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (attemptId: string, answersJson: string, options: any, command: any) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      let answers: Array<{ slot: number; answer: string }>;
      try {
        answers = JSON.parse(answersJson);
      } catch {
        apiContext.log.error("Invalid answers JSON. Expected format: [{\"slot\":1,\"answer\":\"0\"},...]");
        process.exitCode = 1;
        return;
      }

      try {
        // Get attempt data to find uniqueid and sequencecheck values
        const attemptData = await getAllQuizAttemptDataApi(
          apiContext.session,
          parseInt(attemptId)
        );

        const uniqueId = attemptData.attempt.uniqueid ?? attemptData.attempt.attemptid;

        const sequenceChecks = new Map<number, number>();
        for (const q of Object.values(attemptData.questions)) {
          if (q.sequencecheck !== undefined) {
            sequenceChecks.set(q.slot, q.sequencecheck);
          }
        }

        const result = await processQuizAttemptApi(
          apiContext.session,
          parseInt(attemptId),
          uniqueId,
          answers,
          sequenceChecks,
          !!options.submit
        );

        apiContext.log.success(`Attempt ${attemptId} state: ${result.state}`);
        formatAndOutput([result] as unknown as Record<string, unknown>[], output, apiContext.log);
      } catch (error) {
        apiContext.log.error(`Failed to submit attempt: ${error instanceof Error ? error.message : String(error)}`);
        process.exitCode = 1;
      }
    });
}
