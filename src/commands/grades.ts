import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi, getCourseGradesApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { loadWsToken } from "../lib/token.ts";
import { formatAndOutput } from "../index.ts";
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

  gradesCmd
    .command("summary")
    .description("Show grade summary across all courses")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

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
    });

  gradesCmd
    .command("course")
    .description("Show detailed grades for a specific course")
    .argument("<course-id>", "Course ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

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
    });
}
