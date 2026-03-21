import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, SessionInfo, OutputFormat } from "../lib/types.ts";
import { getEnrolledCourses, getEnrolledCoursesApi, getSupervideosInCourse, getSupervideosInCourseApi, getVideoMetadata, completeVideo, downloadVideo } from "../lib/moodle.ts";
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

  // Helper to get output format from global or local options
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

      // Try pure API mode (no browser, fast!)
      const apiContext = await createApiContext(options, command);
      if (apiContext) {
        try {
          const videos = await getSupervideosInCourseApi(apiContext.session, parseInt(courseId, 10));

          // Filter by incomplete only if requested (API returns all, no completion status)
          // Note: API doesn't provide completion status, so --incomplete-only won't work in API mode
          if (options.incompleteOnly) {
            apiContext.log.warn("--incomplete-only is not supported in API mode, showing all videos");
          }

          formatAndOutput(videos as unknown as Record<string, unknown>[], output, apiContext.log);
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
        const videos = await getSupervideosInCourse(page, session, parseInt(courseId, 10), log, {
          incompleteOnly: options.incompleteOnly,
        });

        formatAndOutput(videos as unknown as Record<string, unknown>[], output, log);
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  videosCmd
    .command("complete")
    .description("Complete videos in a course")
    .argument("<course-id>", "Course ID")
    .option("--dry-run", "Discover videos but don't complete them")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;
      const output: OutputFormat = getOutputFormat(command);

      try {
        const videos = await getSupervideosInCourse(page, session, parseInt(courseId, 10), log, {
          incompleteOnly: true,  // Only operate on incomplete videos
        });

        if (videos.length === 0) {
          log.info("所有影片已完成（或無影片）。");
          return;
        }

        const results: Array<{ name: string; success: boolean; error?: string }> = [];

        for (const sv of videos) {
          log.info(`處理中: ${sv.name}`);

          try {
            const video = await getVideoMetadata(page, sv.url, log);

            if (options.dryRun) {
              log.info(`  [試執行] viewId=${video.viewId}, duration=${video.duration}s`);
              results.push({ name: sv.name, success: true });
              continue;
            }

            const success = await completeVideo(page, session, { ...video, cmid: sv.cmid }, log);
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
    .description("Complete all incomplete videos across all courses")
    .option("--dry-run", "Discover videos but don't complete them")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;
      const output: OutputFormat = getOutputFormat(command);

      try {
        const courses = await getEnrolledCourses(page, session, log);

        const allResults: Array<{ courseName: string; name: string; success: boolean; error?: string }> = [];
        let totalVideos = 0;
        let totalCompleted = 0;
        let totalFailed = 0;

        for (const course of courses) {
          log.info(`\n======================================`);
          log.info(`課程: ${course.fullname}`);
          log.info(`======================================`);

          const videos = await getSupervideosInCourse(page, session, course.id, log);

          if (videos.length === 0) {
            log.info("  所有影片已完成（或無影片）。");
            continue;
          }

          totalVideos += videos.length;

          for (const sv of videos) {
            log.info(`  處理中: ${sv.name}`);

            try {
              const video = await getVideoMetadata(page, sv.url, log);

              if (options.dryRun) {
                log.info(`    [試執行] viewId=${video.viewId}, duration=${video.duration}s`);
                allResults.push({ courseName: course.fullname, name: sv.name, success: true });
                continue;
              }

              const success = await completeVideo(page, session, { ...video, cmid: sv.cmid }, log);
              if (success) {
                log.success(`    已完成！`);
                allResults.push({ courseName: course.fullname, name: sv.name, success: true });
                totalCompleted++;
              } else {
                log.error(`    失敗。`);
                allResults.push({ courseName: course.fullname, name: sv.name, success: false, error: "Failed to complete" });
                totalFailed++;
              }
            } catch (err) {
              const msg = err instanceof Error ? err.message : String(err);
              log.error(`    錯誤: ${msg}`);
              allResults.push({ courseName: course.fullname, name: sv.name, success: false, error: msg });
              totalFailed++;
            }
          }
        }

        log.info("\n===== 執行結果 =====");
        log.info(`掃描課程數: ${courses.length}`);
        log.info(`掃描影片數: ${totalVideos}`);
        log.info(`執行影片數: ${totalCompleted}`);
        if (totalFailed > 0) log.warn(`失敗影片數: ${totalFailed}`);

        if (output !== "silent") {
          formatAndOutput(allResults as unknown as Record<string, unknown>[], output, log);
        }
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  // Helper function to sanitize filename
  function sanitizeFilename(name: string): string {
    // Remove/replace invalid characters
    return name
      .replace(/[<>:"/\\|?*]/g, "_") // Replace invalid chars with underscore
      .replace(/\s+/g, "_") // Replace spaces with underscores
      .substring(0, 200); // Limit length
  }

  videosCmd
    .command("download")
    .description("Download videos from a course")
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

        // Create output directory
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

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          course_id: courseId,
          output_dir: outputDir,
          total_videos: videos.length,
          downloaded: completed,
          failed,
          videos: downloaded,
        };
        console.log(JSON.stringify(output));
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });
}
