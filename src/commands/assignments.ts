import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi, getAssignmentsByCoursesApi, getSubmissionStatusApi, saveSubmissionApi, uploadFileApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { loadWsToken } from "../lib/token.ts";
import { formatAndOutput } from "../index.ts";
import path from "node:path";
import fs from "node:fs";
import readline from "node:readline";

export function registerAssignmentsCommand(program: Command): void {
  const assignmentsCmd = program.command("assignments");
  assignmentsCmd.description("Assignment operations");

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

  assignmentsCmd
    .command("list")
    .description("List assignments in a course")
    .argument("<course-id>", "Course ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const apiAssignments = await getAssignmentsByCoursesApi(apiContext.session, [parseInt(courseId, 10)]);

      // Helper to format timestamp
      const formatDate = (timestamp?: number): string => {
        if (!timestamp || timestamp === 0) return "無期限";
        return new Date(timestamp * 1000).toLocaleString("zh-TW");
      };

      const assignments = apiAssignments.map(a => ({
        id: a.id,
        courseName: courseId,
        name: a.name,
        url: a.url,
        cmid: a.cmid,
        duedate: formatDate(a.duedate),
        cutoffdate: formatDate(a.cutoffdate),
        allowSubmissionsFromDate: formatDate(a.allowSubmissionsFromDate),
      }));

      apiContext.log.info(`\n找到 ${assignments.length} 個作業。`);
      formatAndOutput(assignments as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  assignmentsCmd
    .command("list-all")
    .description("List all assignments across all courses")
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

      // Get assignments via WS API (no browser needed!)
      const courseIds = courses.map(c => c.id);
      const apiAssignments = await getAssignmentsByCoursesApi(apiContext.session, courseIds);

      // Build a map of courseId -> course for quick lookup
      const courseMap = new Map(courses.map(c => [c.id, c]));

      // Helper to format timestamp
      const formatDate = (timestamp?: number): string => {
        if (!timestamp || timestamp === 0) return "無期限";
        return new Date(timestamp * 1000).toLocaleString("zh-TW");
      };

      const allAssignments: Array<{
        id: number;
        courseName: string;
        name: string;
        url: string;
        cmid: string;
        duedate: string;
        cutoffdate: string;
        allowSubmissionsFromDate: string;
      }> = [];
      for (const a of apiAssignments) {
        const course = courseMap.get(a.courseId);
        if (course) {
          allAssignments.push({
            id: a.id,
            courseName: course.fullname,
            name: a.name,
            url: a.url,
            cmid: a.cmid,
            duedate: formatDate(a.duedate),
            cutoffdate: formatDate(a.cutoffdate),
            allowSubmissionsFromDate: formatDate(a.allowSubmissionsFromDate),
          });
        }
      }

      apiContext.log.info(`\n總計發現 ${allAssignments.length} 個作業。`);
      formatAndOutput(allAssignments as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  // ── Submission Status ───────────────────────────────────────────────────────

  assignmentsCmd
    .command("status")
    .description("Check assignment submission status")
    .argument("<assignment-id>", "Assignment instance ID (from list-all)")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (assignmentId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const id = parseInt(assignmentId, 10);

      apiContext.log.info("檢查繳交狀態...");
      const status = await getSubmissionStatusApi(apiContext.session, id);

      // Build status data object
      const statusData = {
        submitted: status.submitted,
        submitted_text: status.submitted ? "已繳交" : "尚未繳交",
        graded: status.graded,
        graded_text: status.graded ? "已評分" : "尚未評分",
        last_modified: status.lastModified ? new Date(status.lastModified * 1000).toISOString() : null,
        last_modified_text: status.lastModified ? new Date(status.lastModified * 1000).toLocaleString("zh-TW") : null,
        grader: status.grader,
        grade: status.grade,
        feedback: status.feedback,
        files: status.extensions.map(f => ({
          filename: f.filename,
          filesize: f.filesize,
          filesize_kb: (f.filesize / 1024).toFixed(2),
        })),
      };

      formatAndOutput(statusData as unknown as Record<string, unknown>, output, apiContext.log);
    });

  // ── Submit Assignment ────────────────────────────────────────────────────────

  assignmentsCmd
    .command("submit")
    .description("Submit an assignment (online text or file)")
    .argument("<assignment-id>", "Assignment instance ID (from list-all)")
    .option("--text <content>", "Online text content to submit")
    .option("--file-id <id>", "Draft file ID from file upload")
    .option("--file <path>", "Upload and submit a file directly")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (assignmentId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const id = parseInt(assignmentId, 10);

      // Check submission status first
      const status = await getSubmissionStatusApi(apiContext.session, id);

      let fileUploaded: { filename: string; filesize: number; filesize_kb: string; draft_id: number } | undefined;
      let cancelled = false;

      if (status.submitted) {
        const confirm = await promptConfirm("此作業已經繳交！確定要重新繳交嗎？(y/N): ");
        if (!confirm) {
          cancelled = true;
        }
      }

      if (cancelled) {
        const cancelResult = {
          success: false,
          cancelled: true,
          message: "Submission cancelled by user",
        };
        formatAndOutput(cancelResult as unknown as Record<string, unknown>, output, apiContext.log);
        return;
      }

      // Validate options
      if (!options.text && !options.fileId && !options.file) {
        const errorResult = {
          success: false,
          error: "請提供 --text、--file-id 或 --file 選項。",
        };
        formatAndOutput(errorResult as unknown as Record<string, unknown>, output, apiContext.log);
        process.exitCode = 1;
        return;
      }

      let fileId = options.fileId ? parseInt(options.fileId, 10) : undefined;

      // Upload file if --file option is provided
      if (options.file) {
        const resolvedPath = path.resolve(options.file);

        // Check if file exists
        if (!fs.existsSync(resolvedPath)) {
          const errorResult = {
            success: false,
            error: `檔案不存在: ${options.file}`,
          };
          formatAndOutput(errorResult as unknown as Record<string, unknown>, output, apiContext.log);
          process.exitCode = 1;
          return;
        }

        const stats = fs.statSync(resolvedPath);
        const fileSizeKB = (stats.size / 1024).toFixed(2);

        const uploadResult = await uploadFileApi(apiContext.session, resolvedPath);

        if (!uploadResult.success) {
          const errorResult = {
            success: false,
            error: `檔案上傳失敗: ${uploadResult.error}`,
          };
          formatAndOutput(errorResult as unknown as Record<string, unknown>, output, apiContext.log);
          process.exitCode = 1;
          return;
        }

        fileId = uploadResult.draftId;
        fileUploaded = {
          filename: path.basename(resolvedPath),
          filesize: stats.size,
          filesize_kb: fileSizeKB,
          draft_id: fileId as number,
        };
      }

      // Submit
      const result = await saveSubmissionApi(apiContext.session, id, {
        onlineText: options.text ? { text: options.text } : undefined,
        fileId: fileId,
      });

      const submitResult = {
        success: result.success,
        assignment_id: id,
        submitted: !!result.success,
        online_text: !!options.text,
        file_uploaded: fileUploaded,
        file_id: fileId ?? null,
        error: result.success ? undefined : result.error,
        message: result.success ? "Assignment submitted successfully" : result.error,
      };

      formatAndOutput(submitResult as unknown as Record<string, unknown>, output, apiContext.log);

      if (!result.success) {
        process.exitCode = 1;
      }
    });
}

/**
 * Prompt user for yes/no confirmation.
 */
async function promptConfirm(prompt: string): Promise<boolean> {
  const readline = await import("node:readline");
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });

  return new Promise((resolve) => {
    rl.question(prompt, (answer) => {
      rl.close();
      resolve(/^y/i.test(answer));
    });
  });
}
