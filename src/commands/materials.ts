import { getBaseDir, getOutputFormat, shouldSilenceLogs, sanitizeFilename, getSessionPath, formatFileSize } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, SessionInfo, OutputFormat } from "../lib/types.ts";
import { getEnrolledCourses, getEnrolledCoursesApi, getResourcesByCoursesApi, updateActivityCompletionStatusManually, getSiteInfoApi, moodleApiCall } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { launchAuthenticated, createApiContext } from "../lib/auth.ts";
import { extractSessionInfo } from "../lib/session.ts";
import { closeBrowserSafely } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import { loadWsToken } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

interface MaterialWithCourse {
  course_id: number;
  course_name: string;
  cmid: string;
  name: string;
  url: string;
  modType: string;
  mimetype?: string;
  filesize?: number;
  modified?: number;
}

interface DownloadedFile {
  filename: string;
  path: string;
  size: number;
  course_id: number;
  course_name: string;
}

export function registerMaterialsCommand(program: Command): void {
  const materialsCmd = program.command("materials");
  materialsCmd.description("Material/resource operations");

  // Helper function to create session context (for download commands)
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

    const sessionPath = getSessionPath();

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

  // Helper to download a single resource
  async function downloadResource(
    page: import("playwright-core").Page,
    resource: MaterialWithCourse,
    outputDir: string,
    log: Logger
  ): Promise<DownloadedFile | null> {
    try {
      // Only download resource type (skip url)
      if (resource.modType !== "resource") {
        log.debug(`  Skipping ${resource.modType}: ${resource.name}`);
        return null;
      }

      // Create course directory
      const courseDir = path.join(outputDir, sanitizeFilename(resource.course_name));
      await fs.promises.mkdir(courseDir, { recursive: true });

      // Navigate to resource page
      log.debug(`  Downloading: ${resource.name}`);
      await page.goto(resource.url, { waitUntil: "domcontentloaded", timeout: 30000 });

      // Try to find download link on the page
      const downloadLinks = await page.$$eval('a[href*="forcedownload=1"]', (links) =>
        links.map((a) => (a as HTMLAnchorElement).href)
      );

      if (downloadLinks.length === 0) {
        log.warn(`    No download link found for: ${resource.name}`);
        return null;
      }

      // Download the first available file
      const downloadUrl = downloadLinks[0];

      // Extract filename from URL or use resource name
      const urlObj = new URL(downloadUrl);
      const filenameParam = urlObj.searchParams.get("filename");
      let filename = filenameParam || sanitizeFilename(resource.name);

      // Add extension if missing
      if (resource.mimetype && !path.extname(filename)) {
        const extMap: Record<string, string> = {
          "application/pdf": ".pdf",
          "application/vnd.ms-powerpoint": ".ppt",
          "application/vnd.openxmlformats-officedocument.presentationml.presentation": ".pptx",
          "application/msword": ".doc",
          "application/vnd.openxmlformats-officedocument.wordprocessingml.document": ".docx",
          "application/vnd.ms-excel": ".xls",
          "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet": ".xlsx",
          "application/zip": ".zip",
          "image/jpeg": ".jpg",
          "image/png": ".png",
        };
        if (extMap[resource.mimetype]) {
          filename += extMap[resource.mimetype];
        }
      }

      const outputPath = path.join(courseDir, filename);

      // Trigger download
      const downloadPromise = page.waitForEvent("download", { timeout: 30000 });
      await page.goto(downloadUrl, { waitUntil: "domcontentloaded" });
      const download = await downloadPromise;

      // Save file
      await download.saveAs(outputPath);

      const stats = await fs.promises.stat(outputPath);
      log.success(`    Downloaded: ${filename} (${formatFileSize(stats.size, 1)} KB)`);

      return {
        filename,
        path: outputPath,
        size: stats.size,
        course_id: resource.course_id,
        course_name: resource.course_name,
      };
    } catch (err) {
      log.warn(`    Failed to download ${resource.name}: ${err instanceof Error ? err.message : String(err)}`);
      return null;
    }
  }

  materialsCmd
    .command("list-all")
    .description("List all materials/resources across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const classification = options.level === "all" ? undefined : "inprogress";
      const courses = await getEnrolledCoursesApi(apiContext.session, {
        classification,
      });

      // Get materials via WS API (no browser needed!)
      const courseIds = courses.map(c => c.id);
      const apiResources = await getResourcesByCoursesApi(apiContext.session, courseIds);

      // Build a map of courseId -> course for quick lookup
      const courseMap = new Map(courses.map(c => [c.id, c]));

      const allMaterials: MaterialWithCourse[] = [];
      for (const resource of apiResources) {
        const course = courseMap.get(resource.courseId);
        if (course) {
          allMaterials.push({
            course_id: resource.courseId,
            course_name: course.fullname,
            cmid: resource.cmid,
            name: resource.name,
            url: resource.url,
            modType: resource.modType,
            mimetype: resource.mimetype,
            filesize: resource.filesize,
            modified: resource.modified,
          });
        }
      }

      const output = {
        status: "success",
        timestamp: new Date().toISOString(),
        level: options.level,
        materials: allMaterials.map(m => ({
          course_id: m.course_id,
          course_name: m.course_name,
          id: m.cmid,
          name: m.name,
          type: m.modType,
          mimetype: m.mimetype,
          filesize: m.filesize,
          modified: m.modified ? new Date(m.modified * 1000).toISOString() : null,
          url: m.url,
        })),
        summary: {
          total_courses: courses.length,
          total_materials: allMaterials.length,
          by_type: allMaterials.reduce((acc, m) => {
            acc[m.modType] = (acc[m.modType] || 0) + 1;
            return acc;
          }, {} as Record<string, number>),
        },
      };
      console.log(JSON.stringify(output));
    });

  materialsCmd
    .command("download")
    .description("Download all materials from a specific course (requires browser)")
    .argument("<course-id>", "Course ID")
    .option("--output-dir <path>", "Output directory", "./downloads")
    .action(async (courseId, options, command) => {
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

        // Navigate to course page to find materials
        await page.goto(`https://ilearning.cycu.edu.tw/course/view.php?id=${course.id}`, { waitUntil: "domcontentloaded" });

        // Find all resource links
        const materials: MaterialWithCourse[] = [];
        const resourceLinks = await page.$$eval('a[href*="/mod/resource/view.php"]', (links) => {
          return links.map((a) => ({
            url: (a as HTMLAnchorElement).href,
            name: a.textContent?.trim() || "",
          }));
        });

        for (const link of resourceLinks) {
          const cmidMatch = link.url.match(/id=(\d+)/);
          if (cmidMatch) {
            materials.push({
              course_id: course.id,
              course_name: course.fullname,
              cmid: cmidMatch[1],
              name: link.name,
              url: link.url,
              modType: "resource",
            });
          }
        }

        log.info(`Found ${materials.length} materials in course: ${course.fullname}`);

        const downloadedFiles: DownloadedFile[] = [];
        for (const material of materials) {
          const result = await downloadResource(page, material, options.outputDir, log);
          if (result) {
            downloadedFiles.push(result);
          }
        }

        const summary = {
          total_materials: materials.length,
          downloaded: downloadedFiles.length,
          skipped: materials.length - downloadedFiles.length,
          total_size: downloadedFiles.reduce((sum, f) => sum + f.size, 0),
        };

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          downloaded_files: downloadedFiles.map(f => ({
            filename: f.filename,
            path: f.path,
            size: f.size,
            course_id: f.course_id,
            course_name: f.course_name,
          })),
          summary,
        };
        console.log(JSON.stringify(output));
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  materialsCmd
    .command("download-all")
    .description("Download all materials from all courses (requires browser)")
    .option("--output-dir <path>", "Output directory", "./downloads")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .action(async (options, command) => {
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;

      try {
        const classification = options.level === "all" ? undefined : "inprogress";
        const courses = await getEnrolledCourses(page, session, log, { classification });

        log.info(`Scanning ${courses.length} courses for materials...`);

        const allMaterials: MaterialWithCourse[] = [];
        for (const course of courses) {
          await page.goto(`https://ilearning.cycu.edu.tw/course/view.php?id=${course.id}`, { waitUntil: "domcontentloaded" });

          const resourceLinks = await page.$$eval('a[href*="/mod/resource/view.php"]', (links) => {
            return links.map((a) => ({
              url: (a as HTMLAnchorElement).href,
              name: a.textContent?.trim() || "",
            }));
          });

          for (const link of resourceLinks) {
            const cmidMatch = link.url.match(/id=(\d+)/);
            if (cmidMatch) {
              allMaterials.push({
                course_id: course.id,
                course_name: course.fullname,
                cmid: cmidMatch[1],
                name: link.name,
                url: link.url,
                modType: "resource",
              });
            }
          }
        }

        log.info(`Found ${allMaterials.length} materials across ${courses.length} courses`);

        const downloadedFiles: DownloadedFile[] = [];
        for (const material of allMaterials) {
          const result = await downloadResource(page, material, options.outputDir, log);
          if (result) {
            downloadedFiles.push(result);
          }
        }

        const summary = {
          total_courses: courses.length,
          total_materials: allMaterials.length,
          downloaded: downloadedFiles.length,
          skipped: allMaterials.length - downloadedFiles.length,
          total_size: downloadedFiles.reduce((sum, f) => sum + f.size, 0),
        };

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          downloaded_files: downloadedFiles.map(f => ({
            filename: f.filename,
            path: f.path,
            size: f.size,
            course_id: f.course_id,
            course_name: f.course_name,
          })),
          summary,
        };
        console.log(JSON.stringify(output));
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  materialsCmd
    .command("complete")
    .description("Mark all incomplete resources (non-video) as complete in a course")
    .argument("<course-id>", "Course ID")
    .option("--dry-run", "Show what would be marked complete without doing it")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (courseId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      try {
        const { log, session } = apiContext;

        // Get user ID
        const siteInfo = await getSiteInfoApi(session);

        // Get completion status for all activities in the course
        const completionData = await moodleApiCall<any>(
          session,
          "core_completion_get_activities_completion_status",
          { courseid: parseInt(courseId, 10), userid: siteInfo.userid }
        );

        if (!completionData?.statuses) {
          log.info("No activities found in this course.");
          return;
        }

        // Filter for resources (non-video) that have completion enabled but are not complete
        const incompleteResources = completionData.statuses.filter((status: any) => {
          // Only resources, not supervideo
          if (status.modname === "supervideo") return false;
          // Must have completion enabled
          if (!status.hascompletion) return false;
          // Must be incomplete
          if (status.isoverallcomplete) return false;
          return true;
        });

        if (incompleteResources.length === 0) {
          log.info("All resources are already complete (or no resources with completion tracking).");
          return;
        }

        log.info(`Found ${incompleteResources.length} incomplete resources to complete:`);
        for (const resource of incompleteResources) {
          log.info(`  - ${resource.name} (cmid: ${resource.cmid})`);
        }

        if (options.dryRun) {
          log.info("\n[Dry run] No changes made.");
          return;
        }

        // Mark each resource as complete
        const results: Array<{ cmid: number; name: string; success: boolean }> = [];
        for (const resource of incompleteResources) {
          const success = await updateActivityCompletionStatusManually(session, resource.cmid, true);
          results.push({
            cmid: resource.cmid,
            name: resource.name,
            success,
          });
          if (success) {
            log.success(`  ✓ Completed: ${resource.name}`);
          } else {
            log.error(`  ✗ Failed: ${resource.name}`);
          }
        }

        const completed = results.filter(r => r.success).length;
        const failed = results.filter(r => !r.success).length;
        log.info(`\n執行結果: ${completed} 成功, ${failed} 失敗`);

        if (output !== "silent") {
          formatAndOutput(results as unknown as Record<string, unknown>[], output, log);
        }
      } catch (e) {
        apiContext.log.error(`Error: ${e instanceof Error ? e.message : String(e)}`);
        process.exitCode = 1;
      }
    });

  materialsCmd
    .command("complete-all")
    .description("Mark all incomplete resources (non-video) as complete across all in-progress courses")
    .option("--dry-run", "Show what would be marked complete without doing it")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      try {
        const { log, session } = apiContext;

        // Get user ID
        const siteInfo = await getSiteInfoApi(session);

        // Get all courses
        const classification = options.level === "all" ? undefined : "inprogress";
        const courses = await getEnrolledCoursesApi(session, { classification });

        log.info(`Scanning ${courses.length} courses for incomplete resources...`);

        const allResults: Array<{ courseId: number; courseName: string; cmid: number; name: string; success: boolean }> = [];
        let totalIncomplete = 0;

        for (const course of courses) {
          try {
            // Get completion status for all activities in the course
            const completionData = await moodleApiCall<any>(
              session,
              "core_completion_get_activities_completion_status",
              { courseid: course.id, userid: siteInfo.userid }
            );

            if (!completionData?.statuses) continue;

            // Filter for resources (non-video) that have completion enabled but are not complete
            const incompleteResources = completionData.statuses.filter((status: any) => {
              if (status.modname === "supervideo") return false;
              if (!status.hascompletion) return false;
              if (status.isoverallcomplete) return false;
              return true;
            });

            if (incompleteResources.length > 0) {
              log.info(`\n${course.fullname}: ${incompleteResources.length} incomplete resources`);
              totalIncomplete += incompleteResources.length;

              if (options.dryRun) {
                for (const resource of incompleteResources) {
                  log.info(`  - ${resource.name} (cmid: ${resource.cmid})`);
                }
              } else {
                for (const resource of incompleteResources) {
                  const success = await updateActivityCompletionStatusManually(session, resource.cmid, true);
                  allResults.push({
                    courseId: course.id,
                    courseName: course.fullname,
                    cmid: resource.cmid,
                    name: resource.name,
                    success,
                  });
                  if (success) {
                    log.success(`  ✓ Completed: ${resource.name}`);
                  } else {
                    log.error(`  ✗ Failed: ${resource.name}`);
                  }
                }
              }
            }
          } catch (e) {
            log.warn(`Failed to process course ${course.fullname}: ${e}`);
          }
        }

        if (totalIncomplete === 0) {
          log.info("\nAll resources are already complete (or no resources with completion tracking).");
          return;
        }

        if (options.dryRun) {
          log.info(`\n[Dry run] Found ${totalIncomplete} incomplete resources across ${courses.length} courses.`);
          log.info("Run without --dry-run to mark them as complete.");
          return;
        }

        const completed = allResults.filter(r => r.success).length;
        const failed = allResults.filter(r => !r.success).length;
        log.info(`\n執行結果: ${completed} 成功, ${failed} 失敗`);

        if (output !== "silent") {
          formatAndOutput(allResults as unknown as Record<string, unknown>[], output, log);
        }
      } catch (e) {
        apiContext.log.error(`Error: ${e instanceof Error ? e.message : String(e)}`);
        process.exitCode = 1;
      }
    });
}
