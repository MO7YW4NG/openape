import { getBaseDir, getOutputFormat, shouldSilenceLogs, sanitizeFilename } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, SessionInfo, OutputFormat } from "../lib/types.ts";
import { getEnrolledCourses, getEnrolledCoursesApi, getSupervideosInCourse, getSupervideosInCourseApi, getVideoMetadata, completeVideoApi, completeVideo, downloadVideo, getIncompleteVideosApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { launchAuthenticated } from "../lib/auth.ts";
import { extractSessionInfo } from "../lib/session.ts";
import { closeBrowserSafely } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import { loadWsToken } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

export function registerVideosCommand(program: Command): void {
  const videosCmd = program.command("videos");
  videosCmd.description("Video progress operations");

  // Pure API context - no browser required (fast!)
  async function createApiContext(options: { verbose?: boolean; headed?: boolean }, command?: any): Promise<{
    log: Logger;
    session: { wsToken: string; moodleBaseUrl: string };
  } | null> {
    const opts = command?.optsWithGlobals ? command.optsWithGlobals() : options;
    // Don't silence logs for commands that don't have explicit output format control
    const outputFormat = command && command.optsWithGlobals ? getOutputFormat(command) : "table";
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

  // Helper function to create session context (for browser-only commands)
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

  videosCmd
    .command("list")
    .description("List videos in a course")
    .argument("<course-id>", "Course ID")
    .option("--incomplete-only", "Show only incomplete videos")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      let videos = await getSupervideosInCourseApi(apiContext.session, parseInt(courseId, 10));

      // Filter for incomplete videos if requested
      if (options.incompleteOnly) {
        videos = videos.filter(v => !v.isComplete);
      }

      formatAndOutput(videos as unknown as Record<string, unknown>[], output, apiContext.log);
    });

  videosCmd
    .command("complete")
    .description("Complete videos in a course (uses API for list & completion, browser for metadata)")
    .argument("<course-id>", "Course ID")
    .option("--dry-run", "Discover videos but don't complete them")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);

      // Get API context for getting incomplete videos and completion
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      // Get incomplete videos via API (fast, no browser needed)
      const incompleteVideos = await getIncompleteVideosApi(apiContext.session, parseInt(courseId, 10));

      if (incompleteVideos.length === 0) {
        apiContext.log.info("所有影片已完成（或無影片）。");
        return;
      }

      apiContext.log.info(`找到 ${incompleteVideos.length} 部未完成影片`);

      // Dry-run: show videos without needing browser
      if (options.dryRun) {
        const results = incompleteVideos.map(v => ({ name: v.name, success: true }));
        for (const video of incompleteVideos) {
          apiContext.log.info(`  [試執行] ${video.name}`);
        }
        apiContext.log.info(`\n執行結果: ${results.length} 影片將被完成`);

        if (output !== "silent") {
          formatAndOutput(results as unknown as Record<string, unknown>[], output, apiContext.log);
        }
        return;
      }

      // Need browser only for getting viewId and duration (not needed for dry-run)
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, browser, context: browserContext } = context;

      try {
        const results: Array<{ name: string; success: boolean; error?: string }> = [];

        for (const sv of incompleteVideos) {
          log.info(`處理中: ${sv.name}`);

          try {
            const video = await getVideoMetadata(page, sv.url, log);

            // Use WS API for completion
            const success = await completeVideoApi(apiContext.session, { ...video, cmid: sv.cmid.toString() });
            if (success) {
              log.success(`  已完成！`);
              results.push({ name: sv.name, success: true });
            } else {
              log.error(`  失敗。`);
              results.push({ name: sv.name, success: false, error: "Failed to complete" });
            }
          } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            log.error(`  錯誤: ${msg}`);
            results.push({ name: sv.name, success: false, error: msg });
          }
        }

        const completed = results.filter(r => r.success).length;
        const failed = results.filter(r => !r.success).length;
        log.info(`\n執行結果: ${completed} 成功, ${failed} 失敗`);

        if (output !== "silent") {
          formatAndOutput(results as unknown as Record<string, unknown>[], output, log);
        }
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  videosCmd
    .command("complete-all")
    .description("Complete all incomplete videos across all courses (uses API for list & completion, browser for metadata)")
    .option("--dry-run", "Discover videos but don't complete them")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);

      // Get API context for getting incomplete videos and completion
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      // Get all courses via API
      const classification = undefined; // all courses
      const courses = await getEnrolledCoursesApi(apiContext.session, { classification });

      apiContext.log.info(`掃描 ${courses.length} 個課程...`);

      // Collect all incomplete videos across all courses using flatMap for cleaner code
      const allIncompleteVideos = (
        await Promise.allSettled(
          courses.map(async (course) => {
            try {
              const videos = await getIncompleteVideosApi(apiContext.session, course.id);
              return videos.map((video) => ({
                courseId: course.id,
                courseName: course.fullname,
                cmid: video.cmid,
                name: video.name,
                url: video.url,
              }));
            } catch (e) {
              apiContext.log.warn(`無法取得課程 ${course.fullname} 的影片: ${e}`);
              return [] as Array<{ courseId: number; courseName: string; cmid: number; name: string; url: string }>;
            }
          })
        )
      )
        .filter((result) => result.status === "fulfilled")
        .flatMap((result) => result.status === "fulfilled" ? result.value : []);

      if (allIncompleteVideos.length === 0) {
        apiContext.log.info("所有影片已完成（或無影片）。");
        return;
      }

      apiContext.log.info(`找到 ${allIncompleteVideos.length} 部未完成影片`);

      // Dry-run: show videos without needing browser
      if (options.dryRun) {
        for (const video of allIncompleteVideos) {
          apiContext.log.info(`  [試執行] [${video.courseName}] ${video.name}`);
        }
        apiContext.log.info("\n===== 執行結果 =====");
        apiContext.log.info(`掃描課程數: ${courses.length}`);
        apiContext.log.info(`找到未完成影片: ${allIncompleteVideos.length}`);
        apiContext.log.info(`執行影片數: ${allIncompleteVideos.length} (試執行)`);
        return;
      }

      // Need browser only for getting viewId and duration (not needed for dry-run)
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, browser, context: browserContext } = context;

      try {
        const allResults: Array<{ courseName: string; name: string; success: boolean; error?: string }> = [];
        let totalCompleted = 0;
        let totalFailed = 0;

        for (const video of allIncompleteVideos) {
          log.info(`處理中: [${video.courseName}] ${video.name}`);

          try {
            const metadata = await getVideoMetadata(page, video.url, log);

            // Use WS API for completion
            const success = await completeVideoApi(apiContext.session, { ...metadata, cmid: video.cmid.toString() });
            if (success) {
              log.success(`  已完成！`);
              allResults.push({ courseName: video.courseName, name: video.name, success: true });
              totalCompleted++;
            } else {
              log.error(`  失敗。`);
              allResults.push({ courseName: video.courseName, name: video.name, success: false, error: "Failed to complete" });
              totalFailed++;
            }
          } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            log.error(`  錯誤: ${msg}`);
            allResults.push({ courseName: video.courseName, name: video.name, success: false, error: msg });
            totalFailed++;
          }
        }

        log.info("\n===== 執行結果 =====");
        log.info(`掃描課程數: ${courses.length}`);
        log.info(`找到未完成影片: ${allIncompleteVideos.length}`);
        log.info(`執行影片數: ${totalCompleted}`);
        if (totalFailed > 0) log.warn(`失敗影片數: ${totalFailed}`);

        if (output !== "silent") {
          formatAndOutput(allResults as unknown as Record<string, unknown>[], output, log);
        }
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  videosCmd
    .command("download")
    .description("Download videos from a course (requires browser)")
    .argument("<course-id>", "Course ID")
    .option("--output-dir <path>", "Output directory", "./downloads/videos")
    .option("--incomplete-only", "Download only incomplete videos")
    .action(async (courseId, options, command) => {
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;

      try {
        const videos = await getSupervideosInCourse(page, session, parseInt(courseId, 10), log, {
          incompleteOnly: options.incompleteOnly,
        });

        log.info(`找到 ${videos.length} 個影片`);

        const baseDir = getBaseDir();
        const outputDir = path.resolve(baseDir, options.outputDir);
        fs.mkdirSync(outputDir, { recursive: true });

        const downloaded: Array<{ name: string; path: string; success: boolean; error?: string; type?: string }> = [];

        for (const video of videos) {
          const filename = sanitizeFilename(video.name) + ".mp4";
          const outputPath = path.join(outputDir, filename);

          log.info(`處理中: ${video.name}`);

          try {
            const metadata = await getVideoMetadata(page, video.url, log);
            const result = await downloadVideo(page, metadata, outputPath, log);

            if (result.success) {
              log.success(`  已下載: ${result.path}`);
              downloaded.push({ name: video.name, path: result.path!, success: true, type: result.type });
            } else {
              log.warn(`  失敗: ${result.error}`);
              downloaded.push({ name: video.name, path: "", success: false, error: result.error, type: result.type });
            }
          } catch (err) {
            const msg = err instanceof Error ? err.message : String(err);
            log.error(`  錯誤: ${msg}`);
            downloaded.push({ name: video.name, path: "", success: false, error: msg });
          }
        }

        const completed = downloaded.filter(d => d.success).length;
        const failed = downloaded.filter(d => !d.success).length;
        log.info(`\n執行結果: ${completed} 成功, ${failed} 失敗`);

        console.log(JSON.stringify({
          status: "success",
          timestamp: new Date().toISOString(),
          course_id: courseId,
          output_dir: outputDir,
          total_videos: videos.length,
          downloaded: completed,
          failed,
        }));
        for (const v of downloaded) {
          console.log(JSON.stringify(v));
        }
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });
}
