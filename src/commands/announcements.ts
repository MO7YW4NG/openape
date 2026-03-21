import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, SessionInfo, OutputFormat } from "../lib/types.ts";
import { getEnrolledCourses, getEnrolledCoursesApi, getForumsInCourse, getForumDiscussions, getDiscussionPosts, getSiteInfoApi, getMessagesApi, type Message } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { launchAuthenticated } from "../lib/auth.ts";
import { extractSessionInfo } from "../lib/session.ts";
import { closeBrowserSafely } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import { loadWsToken } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

interface AnnouncementWithCourse {
  course_id: number;
  course_name: string;
  id: number;
  subject: string;
  author: string;
  authorId: number;
  createdAt: number;
  modifiedAt: number;
  unread?: boolean;
  forumId: number;
}

export function registerAnnouncementsCommand(program: Command): void {
  const announcementsCmd = program.command("announcements");
  announcementsCmd.description("Announcement operations");

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

  announcementsCmd
    .command("list-all")
    .description("List all announcements across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .option("--unread-only", "Show only unread announcements")
    .option("--limit <n>", "Maximum number of announcements to show", "20")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const limit = parseInt(options.limit, 10);

      // Try pure API mode (no browser, fast!)
      const apiContext = await createApiContext(options, command);
      if (apiContext) {
        try {
          // Get site info to retrieve userid
          const siteInfo = await getSiteInfoApi(apiContext.session);

          // Get messages for the current user
          const messages = await getMessagesApi(apiContext.session, siteInfo.userid, {
            limitnum: limit,
          });

          // Convert messages to announcement format
          const allAnnouncements: AnnouncementWithCourse[] = messages.map(m => ({
            course_id: 0, // Messages don't have courseId
            course_name: "Notifications",
            id: m.id,
            subject: m.subject,
            author: `User ${m.useridfrom}`,
            authorId: m.useridfrom,
            createdAt: m.timecreated,
            modifiedAt: m.timecreated,
            unread: false, // Messages API doesn't provide unread status
            forumId: 0,
          }));

          // Sort by created date (newest first)
          allAnnouncements.sort((a, b) => b.createdAt - a.createdAt);

          // Apply limit
          let filteredAnnouncements = allAnnouncements.slice(0, limit);

          const output = {
            status: "success",
            timestamp: new Date().toISOString(),
            level: options.level,
            announcements: filteredAnnouncements.map(a => ({
              course_id: a.course_id,
              course_name: a.course_name,
              id: a.id,
              subject: a.subject,
              author: a.author,
              author_id: a.authorId,
              created_at: new Date(a.createdAt * 1000).toISOString(),
              modified_at: new Date(a.modifiedAt * 1000).toISOString(),
              unread: a.unread,
            })),
            summary: {
              total_announcements: allAnnouncements.length,
              shown: filteredAnnouncements.length,
            },
          };
          console.log(JSON.stringify(output));
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
        const classification = options.level === "all" ? undefined : "inprogress";
        const courses = await getEnrolledCourses(page, session, log, { classification });

        const allAnnouncements: AnnouncementWithCourse[] = [];
        for (const course of courses) {
          const forums = await getForumsInCourse(page, session, course.id, log);

          // Find news/announcement forums (usually named "news" or "Announcements")
          const announcementForums = forums.filter(f =>
            f.forumType === "news" ||
            f.name.toLowerCase().includes("news") ||
            f.name.toLowerCase().includes("announcement") ||
            f.name.toLowerCase().includes("公告")
          );

          for (const forum of announcementForums) {
            try {
              const discussions = await getForumDiscussions(page, session, parseInt(forum.cmid, 10));

              for (const discussion of discussions) {
                // Get the first post to get author info
                let author = "Unknown";
                let authorId = 0;
                let createdAt = discussion.timeModified;

                try {
                  const posts = await getDiscussionPosts(page, session, discussion.id);
                  if (posts.length > 0) {
                    const firstPost = posts[0];
                    author = firstPost.author;
                    authorId = firstPost.authorId || 0;
                    createdAt = firstPost.created;
                  }
                } catch {
                  // Ignore errors fetching posts
                }

                allAnnouncements.push({
                  course_id: course.id,
                  course_name: course.fullname,
                  id: discussion.id,
                  subject: discussion.name,
                  author,
                  authorId,
                  createdAt,
                  modifiedAt: discussion.timeModified,
                  unread: discussion.unread,
                  forumId: parseInt(forum.cmid, 10),
                });
              }
            } catch (err) {
              log.debug(`Failed to fetch announcements for ${course.fullname}: ${err}`);
            }
          }
        }

        // Sort by created date (newest first)
        allAnnouncements.sort((a, b) => b.createdAt - a.createdAt);

        // Apply limit
        let filteredAnnouncements = allAnnouncements.slice(0, limit);

        // Filter unread only if requested
        if (options.unreadOnly) {
          filteredAnnouncements = filteredAnnouncements.filter(a => a.unread);
        }

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          announcements: filteredAnnouncements.map(a => ({
            course_id: a.course_id,
            course_name: a.course_name,
            id: a.id,
            subject: a.subject,
            author: a.author,
            author_id: a.authorId,
            created_at: new Date(a.createdAt * 1000).toISOString(),
            modified_at: new Date(a.modifiedAt * 1000).toISOString(),
            unread: a.unread,
          })),
          summary: {
            total_announcements: allAnnouncements.length,
            unread: allAnnouncements.filter(a => a.unread).length,
            shown: filteredAnnouncements.length,
          },
        };
        console.log(JSON.stringify(output));
      } catch (err) {
        log.error(`Error: ${err}`);
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  announcementsCmd
    .command("read")
    .description("Read a specific announcement (shows full content)")
    .argument("<announcement-id>", "Discussion ID of the announcement")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (announcementId, options, command) => {
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;

      try {
        const posts = await getDiscussionPosts(page, session, parseInt(announcementId, 10));

        if (posts.length === 0) {
          log.error(`Announcement not found: ${announcementId}`);
          process.exitCode = 1;
          return;
        }

        const firstPost = posts[0];

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          announcement: {
            id: announcementId,
            subject: firstPost.subject,
            author: firstPost.author,
            author_id: firstPost.authorId,
            created_at: new Date(firstPost.created * 1000).toISOString(),
            modified_at: new Date(firstPost.modified * 1000).toISOString(),
            message: firstPost.message,
          },
        };
        console.log(JSON.stringify(output));
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });
}
