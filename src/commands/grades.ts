import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, SessionInfo, OutputFormat } from "../lib/types.ts";
import { getEnrolledCourses, getEnrolledCoursesApi, getCourseGrades, getCourseGradesApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { launchAuthenticated } from "../lib/auth.ts";
import { extractSessionInfo } from "../lib/session.ts";
import { closeBrowserSafely } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import { loadWsToken } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

interface GradeSummary {
  courseId: number;
  courseName: string;
  grade?: string;
  gradeFormatted?: string;
  rank?: number;
  totalUsers?: number;
}

export function registerGradesCommand(program: Command): void {
  const gradesCmd = program.command("grades");
  gradesCmd.description("Grade operations");

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
      return null;
    }

    // Try to load WS token
    const wsToken = loadWsToken(sessionPath);
    if (!wsToken) {
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

  // Helper function to create session context
  async function createSessionContext(options: { verbose?: boolean; headed?: boolean }, command?: any): Promise<{
    log: Logger;
    page: import("playwright-core").Page;
    session: SessionInfo;
    browser: any;
    context: any;
  } | null> {
    // Get global options if command is provided (for --verbose, --silent flags)
    const opts = command?.optsWithGlobals ? command.optsWithGlobals() : options;
    // Auto-enable silent mode for JSON output (unless --verbose is also set)
    const outputFormat = getOutputFormat(command || { optsWithGlobals: () => ({ output: "json" }) });
    const silent = outputFormat === "json" && !opts.verbose;
    const log = createLogger(opts.verbose, silent);

    // Determine session path
    const baseDir = getBaseDir();
    const sessionPath = path.resolve(baseDir, ".auth", "storage-state.json");

    // Check if session exists
    if (!fs.existsSync(sessionPath)) {
      log.error("未找到登入 session。請先執行 'openape auth login' 進行登入。");
      log.info(`Session 預期位置: ${sessionPath}`);
      return null;
    }

    // Create minimal config
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

  gradesCmd
    .command("summary")
    .description("Show grade summary across all courses")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);

      // Try pure API mode (no browser, fast!)
      const apiContext = await createApiContext(options, command);
      if (apiContext) {
        try {
          const courses = await getEnrolledCoursesApi(apiContext.session);

          const gradeSummaries: GradeSummary[] = [];
          for (const course of courses) {
            const grades = await getCourseGradesApi(apiContext.session, course.id);
            gradeSummaries.push({
              courseId: course.id,
              courseName: course.fullname,
              grade: grades.grade,
              gradeFormatted: grades.gradeFormatted,
              rank: grades.rank,
              totalUsers: grades.totalUsers,
            });
          }

          // Calculate overall statistics
          const gradedCourses = gradeSummaries.filter(g => g.grade !== undefined && g.grade !== null && g.grade !== "-");
          const averageRank = gradeSummaries
            .filter(g => g.rank !== undefined && g.rank !== null)
            .reduce((sum, g) => sum + (g.rank || 0), 0) /
            (gradeSummaries.filter(g => g.rank !== undefined && g.rank !== null).length || 1);

          const summaryData = {
            total_courses: courses.length,
            graded_courses: gradedCourses.length,
            average_rank: averageRank.toFixed(1),
            grades: gradeSummaries,
          };

          formatAndOutput(summaryData as unknown as Record<string, unknown>, output, apiContext.log);
          return;
        } catch (e) {
          // API failed, fall through to browser mode
          const msg = e instanceof Error ? e.message : String(e);
          console.error(`// API mode failed: ${msg}, trying browser mode...`);
        }
      }

      // Fallback to browser mode
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;

      try {
        const courses = await getEnrolledCourses(page, session, log);

        const gradeSummaries: GradeSummary[] = [];
        for (const course of courses) {
          const grades = await getCourseGrades(page, session, course.id);
          gradeSummaries.push({
            courseId: course.id,
            courseName: course.fullname,
            grade: grades.grade,
            gradeFormatted: grades.gradeFormatted,
            rank: grades.rank,
            totalUsers: grades.totalUsers,
          });
        }

        // Calculate overall statistics
        const gradedCourses = gradeSummaries.filter(g => g.grade !== undefined && g.grade !== null && g.grade !== "-");
        const averageRank = gradeSummaries
          .filter(g => g.rank !== undefined && g.rank !== null)
          .reduce((sum, g) => sum + (g.rank || 0), 0) /
          (gradeSummaries.filter(g => g.rank !== undefined && g.rank !== null).length || 1);

        const summaryData = {
          total_courses: courses.length,
          graded_courses: gradedCourses.length,
          average_rank: averageRank.toFixed(1),
          grades: gradeSummaries,
        };

        formatAndOutput(summaryData as unknown as Record<string, unknown>, output, log);
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  gradesCmd
    .command("course")
    .description("Show detailed grades for a specific course")
    .argument("<course-id>", "Course ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);

      // Try pure API mode (no browser, fast!)
      const apiContext = await createApiContext(options, command);
      if (apiContext) {
        try {
          const courses = await getEnrolledCoursesApi(apiContext.session);
          const course = courses.find(c => c.id === parseInt(courseId, 10));

          if (!course) {
            apiContext.log.error(`Course not found: ${courseId}`);
            process.exitCode = 1;
            return;
          }

          const grades = await getCourseGradesApi(apiContext.session, course.id);

          const gradeData = {
            courseId: grades.courseId,
            courseName: grades.courseName,
            grade: grades.grade,
            gradeFormatted: grades.gradeFormatted,
            rank: grades.rank,
            totalUsers: grades.totalUsers,
            items: grades.items?.map(item => ({
              name: item.name,
              grade: item.grade,
              gradeFormatted: item.gradeFormatted,
              range: item.range,
              percentage: item.percentage,
              weight: item.weight,
              feedback: item.feedback,
              graded: item.graded,
            })),
          };

          formatAndOutput(gradeData as unknown as Record<string, unknown>, output, apiContext.log);
          return;
        } catch (e) {
          // API failed, fall through to browser mode
          const msg = e instanceof Error ? e.message : String(e);
          console.error(`// API mode failed: ${msg}, trying browser mode...`);
        }
      }

      // Fallback to browser mode
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;

      try {
        const courses = await getEnrolledCourses(page, session, log);
        const course = courses.find(c => c.id === parseInt(courseId, 10));

        if (!course) {
          log.error(`Course not found: ${courseId}`);
          process.exitCode = 1;
          return;
        }

        const grades = await getCourseGrades(page, session, course.id);

        const gradeData = {
          courseId: grades.courseId,
          courseName: grades.courseName,
          grade: grades.grade,
          gradeFormatted: grades.gradeFormatted,
          rank: grades.rank,
          totalUsers: grades.totalUsers,
          items: grades.items?.map(item => ({
            name: item.name,
            grade: item.grade,
            gradeFormatted: item.gradeFormatted,
            range: item.range,
            percentage: item.percentage,
            weight: item.weight,
            feedback: item.feedback,
            graded: item.graded,
          })),
        };

        formatAndOutput(gradeData as unknown as Record<string, unknown>, output, log);
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });
}
