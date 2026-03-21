import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, SessionInfo, OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi, getQuizzesByCoursesApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { launchAuthenticated } from "../lib/auth.ts";
import { extractSessionInfo } from "../lib/session.ts";
import { closeBrowserSafely } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import { loadWsToken } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

export function registerQuizzesCommand(program: Command): void {
  const quizzesCmd = program.command("quizzes");
  quizzesCmd.description("Quiz operations");

  function getOutputFormat(command: any): OutputFormat {
    const opts = command.optsWithGlobals();
    return (opts.output as OutputFormat) || "json";
  }

  // Pure API context - no browser required (fast!)
  async function createApiContext(options: { verbose?: boolean; headed?: boolean }, command?: any): Promise<{
    log: Logger;
    session: { wsToken: string; moodleBaseUrl: string };
  } | null> {
    const opts = command?.optsWithGlobals ? command.optsWithGlobals() : options;
    const outputFormat = getOutputFormat(command || { optsWithGlobals: () => ({ output: "json" }) });
    const silent = outputFormat === "json" && !opts.verbose;
    const log = createLogger(opts.verbose, silent);

    const baseDir = getBaseDir();
    const sessionPath = path.resolve(baseDir, ".auth", "storage-state.json");

    // Check if session exists
    if (!fs.existsSync(sessionPath)) {
      log.error("未找到登入 session。請先執行 'openape auth login' 進行登入。");
      log.info(`Session 預期位置: ${sessionPath}`);
      return null;
    }

    // Try to load WS token
    const wsToken = loadWsToken(sessionPath);
    if (!wsToken) {
      log.error("未找到 WS token。請先執行 'openape auth login' 進行登入。");
      return null;
    }

    return {
      log,
      session: {
        wsToken,
        moodleBaseUrl: "https://ilearning.cycu.edu.tw",
      },
    };
  }

  // Helper function to create session context (for open command only)
  async function createSessionContext(options: { verbose?: boolean; headed?: boolean }, command?: any): Promise<{
    log: Logger;
    page: import("playwright-core").Page;
    session: SessionInfo;
    browser: any;
    context: any;
  } | null> {
    const opts = command?.optsWithGlobals ? command.optsWithGlobals() : options;
    const outputFormat = getOutputFormat(command || { optsWithGlobals: () => ({ output: "json" }) });
    const silent = outputFormat === "json" && !opts.verbose;
    const log = createLogger(opts.verbose, silent);

    const baseDir = getBaseDir();
    const sessionPath = path.resolve(baseDir, ".auth", "storage-state.json");

    if (!fs.existsSync(sessionPath)) {
      log.error("未找到登入 session。請先執行 'openape auth login' 進行登入。");
      return null;
    }

    const config = {
      username: "",
      password: "",
      courseUrl: "",
      moodleBaseUrl: "https://ilearning.cycu.edu.tw",
      headless: !options.headed,
      slowMo: 0,
      authStatePath: sessionPath,
      ollamaBaseUrl: "",
    };

    log.info("啟動瀏覽器...");
    const { browser, context, page } = await launchAuthenticated(config, log);

    try {
      const session = await extractSessionInfo(page, config, log);
      return { log, page, session, browser, context };
    } catch (err) {
      await context.close();
      await browser.close();
      throw err;
    }
  }

  quizzesCmd
    .command("list")
    .description("List quizzes in a course")
    .argument("<course-id>", "Course ID")
    .option("--available-only", "Show only available quizzes")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const quizzes = await getQuizzesByCoursesApi(apiContext.session, [parseInt(courseId, 10)]);

      // Note: API doesn't provide completion status, so --available-only shows all
      if (options.availableOnly) {
        apiContext.log.warn("--available-only is not supported in API mode, showing all quizzes");
      }

      formatAndOutput(quizzes as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  quizzesCmd
    .command("list-all")
    .description("List all available quizzes across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
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

      const allQuizzes: Array<{ courseName: string; name: string; url: string; cmid: string; isComplete: boolean }> = [];
      for (const q of apiQuizzes) {
        const course = courseMap.get(q.courseId);
        if (course) {
          allQuizzes.push({
            courseName: course.fullname,
            name: q.name,
            url: q.url,
            cmid: q.cmid,
            isComplete: q.isComplete,
          });
        }
      }

      apiContext.log.info(`\n總計發現 ${allQuizzes.length} 個測驗。`);
      formatAndOutput(allQuizzes as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  quizzesCmd
    .command("open")
    .description("Open a quiz URL in browser (manual mode)")
    .argument("<quiz-url>", "Quiz URL")
    .option("--headed", "Run browser in visible mode (default: true)")
    .action(async (quizUrl, options, command) => {
      const context = await createSessionContext({ ...options, headed: true }, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, browser, context: browserContext } = context;

      try {
        log.info(`導航至測驗頁面: ${quizUrl}`);
        await page.goto(quizUrl, { waitUntil: "domcontentloaded" });

        log.info("瀏覽器已開啟，請手動完成測驗。");
        log.info("按 Ctrl+C 關閉瀏覽器。");

        await new Promise(() => {});
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });
}
