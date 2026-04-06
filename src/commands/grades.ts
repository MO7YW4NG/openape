import { Command } from "commander";
import type { OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi, getCourseGradesApi } from "../lib/moodle.ts";
import { createApiContext } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import { getOutputFormat } from "../lib/utils.ts";

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

      const gradeResults = await Promise.allSettled(
        courses.map(course =>
          getCourseGradesApi(apiContext.session, course.id)
            .then(grades => ({ course, grades }))
        )
      );

      const gradeSummaries: GradeSummary[] = [];
      for (const result of gradeResults) {
        if (result.status !== "fulfilled") continue;
        const { course, grades } = result.value;
        gradeSummaries.push({
          courseId: course.id,
          courseName: course.fullname,
          grade: grades.grade,
          gradeFormatted: grades.gradeFormatted,
          rank: grades.rank,
          totalUsers: grades.totalUsers,
        });
      }

      const gradedCourses = gradeSummaries.filter(g => g.grade !== undefined && g.grade !== null && g.grade !== "-");
      const averageRank = gradeSummaries
        .filter(g => g.rank !== undefined && g.rank !== null)
        .reduce((sum, g) => sum + (g.rank || 0), 0) /
        (gradeSummaries.filter(g => g.rank !== undefined && g.rank !== null).length || 1);

      apiContext.log.info(`Total: ${courses.length} courses, ${gradedCourses.length} graded, avg rank: ${averageRank.toFixed(1)}`);

      formatAndOutput(gradeSummaries as unknown as Record<string, unknown>[], output, apiContext.log);
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
