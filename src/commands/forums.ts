import { getBaseDir } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, SessionInfo, OutputFormat } from "../lib/types.ts";
import { getEnrolledCourses, getEnrolledCoursesApi, getForumsInCourse, getForumsApi, getForumDiscussions, getDiscussionPosts, getForumIdFromPage } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { launchAuthenticated } from "../lib/auth.ts";
import { extractSessionInfo } from "../lib/session.ts";
import { closeBrowserSafely } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";
import { loadWsToken } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

interface ForumWithCourse {
  course_id: number;
  course_name: string;
  cmid: string;
  forum_id: number;
  name: string;
  url: string;
}

interface DiscussionWithForum {
  forum_id: number;
  forum_name: string;
  id: number;
  name: string;
  userId: number;
  timedue?: number;
  timeModified: number;
  postCount?: number;
  unread?: boolean;
}

export function registerForumsCommand(program: Command): void {
  const forumsCmd = program.command("forums");
  forumsCmd.description("Forum operations");

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
    const { browser, context, page, wsToken } = await launchAuthenticated(config, log);

    try {
      const session = await extractSessionInfo(page, config, log, wsToken);
      return { log, page, session, browser, context };
    } catch (err) {
      await context.close();
      await browser.close();
      throw err;
    }
  }

  forumsCmd
    .command("list")
    .description("List forums from in-progress courses")
    .option("--unread-only", "Show only forums with unread discussions")
    .option("--fetch-instance", "Fetch forum instance IDs (slower)")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);

      // Try pure WS API mode (no browser, fast!)
      const apiContext = await createApiContext(options, command);
      if (apiContext) {
        try {
          const courses = await getEnrolledCoursesApi(apiContext.session, {
            classification: "inprogress",
          });

          // Get forums via WS API (no browser needed!)
          const courseIds = courses.map(c => c.id);
          const wsForums = await getForumsApi(apiContext.session, courseIds);

          const allForums: ForumWithCourse[] = [];
          for (const wsForum of wsForums) {
            const course = courses.find(c => c.id === wsForum.courseid);
            if (course) {
              allForums.push({
                course_id: wsForum.courseid,
                course_name: course.fullname,
                cmid: wsForum.cmid.toString(),
                forum_id: wsForum.id,
                name: wsForum.name,
                url: `https://ilearning.cycu.edu.tw/mod/forum/view.php?id=${wsForum.cmid}`,
              });
            }
          }

          const result = {
            status: "success",
            timestamp: new Date().toISOString(),
            forums: allForums,
            summary: {
              total_courses: courses.length,
              total_forums: allForums.length,

            },
          };

          console.log(JSON.stringify(result));
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
        const courses = await getEnrolledCourses(page, session, log, {
          classification: "inprogress",
        });

        const allForums: ForumWithCourse[] = [];
        for (const course of courses) {
          const forums = await getForumsInCourse(page, session, course.id, log);
          for (const forum of forums) {
            let instance = forum.forumId;

            // Fetch instance ID if requested
            if (options.fetchInstance) {
              log.info(`  正在取得 forum ${forum.cmid} 的 instance ID...`);
              instance = await getForumIdFromPage(page, parseInt(forum.cmid, 10), session) ?? 0;
            }

            allForums.push({
              course_id: course.id,
              course_name: course.fullname,
              cmid: forum.cmid,
              forum_id: instance,
              name: forum.name,
              url: forum.url,
            });
          }
        }

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          forums: allForums.map(f => ({
            course_id: f.course_id,
            course_name: f.course_name,
            cmid: f.cmid,
            forum_id: f.forum_id,
            name: f.name,
            url: f.url,
          })),
          summary: {
            total_courses: courses.length,
            total_forums: allForums.length,
          },
        };
        console.log(JSON.stringify(output));
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  forumsCmd
    .command("list-all")
    .description("List all forums across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .option("--unread-only", "Show only forums with unread discussions")
    .option("--fetch-instance", "Fetch forum instance IDs (slower)")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      // Try pure WS API mode (no browser, fast!)
      const apiContext = await createApiContext(options, command);
      if (apiContext) {
        try {
          const classification = options.level === "all" ? undefined : "inprogress";
          const courses = await getEnrolledCoursesApi(apiContext.session, {
            classification,
          });

          // Get forums via WS API (no browser needed!)
          const courseIds = courses.map(c => c.id);
          const wsForums = await getForumsApi(apiContext.session, courseIds);

          const allForums: ForumWithCourse[] = [];
          for (const wsForum of wsForums) {
            const course = courses.find(c => c.id === wsForum.courseid);
            if (course) {
              allForums.push({
                course_id: wsForum.courseid,
                course_name: course.fullname,
                cmid: wsForum.cmid.toString(),
                forum_id: wsForum.id,
                name: wsForum.name,
                url: `https://ilearning.cycu.edu.tw/mod/forum/view.php?id=${wsForum.cmid}`,
              });
            }
          }

          const result = {
            status: "success",
            timestamp: new Date().toISOString(),
            forums: allForums,
            summary: {
              total_courses: courses.length,
              total_forums: allForums.length,
            },
          };

          console.log(JSON.stringify(result));
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

        const allForums: ForumWithCourse[] = [];
        for (const course of courses) {
          const forums = await getForumsInCourse(page, session, course.id, log);
          for (const forum of forums) {
            let instance = forum.forumId;

            // Fetch instance ID if requested
            if (options.fetchInstance) {
              log.info(`  正在取得 forum ${forum.cmid} 的 instance ID...`);
              instance = await getForumIdFromPage(page, parseInt(forum.cmid, 10), session) ?? 0;
            }

            allForums.push({
              course_id: course.id,
              course_name: course.fullname,
              cmid: forum.cmid,
              forum_id: instance,
              name: forum.name,
              url: forum.url,
            });
          }
        }

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          forums: allForums.map(f => ({
            course_id: f.course_id,
            course_name: f.course_name,
            cmid: f.cmid,
            forum_id: f.forum_id,
            name: f.name,
            url: f.url,
          })),
          summary: {
            total_courses: courses.length,
            total_forums: allForums.length,
          },
        };
        console.log(JSON.stringify(output));
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  forumsCmd
    .command("discussions")
    .description("List discussions in a forum (use cmid or instance ID)")
    .argument("<forum-id>", "Forum cmid or instance ID")
    .option("--unread-only", "Show only unread discussions")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (forumId, options, command) => {
      const context = await createSessionContext({ verbose: false }, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;

      try {
        // Find forum by cmid or instance ID
        const courses = await getEnrolledCourses(page, session, log);
        let targetForum: { forumId: number; forumName: string } | null = null;

        for (const course of courses) {
          const forums = await getForumsInCourse(page, session, course.id, log);
          const forum = forums.find(f => f.cmid === forumId || f.forumId === parseInt(forumId, 10));
          if (forum) {
            targetForum = { forumId: forum.forumId, forumName: forum.name };
            break;
          }
        }

        if (!targetForum) {
          console.log(JSON.stringify({ status: "error", error: "Forum not found" }));
          process.exitCode = 1;
          return;
        }

        // Use WS API to get discussions
        const discussions = await getForumDiscussions(page, session, targetForum.forumId);

        const output = {
          status: "success",
          timestamp: new Date().toISOString(),
          forum_id: targetForum.forumId,
          forum_name: targetForum.forumName,
          discussions: discussions.map(d => ({
            id: d.id,
            name: d.name,
            user_id: d.userId,
            time_modified: d.timeModified,
            post_count: d.postCount,
            unread: d.unread,
          })),
          summary: {
            total_discussions: discussions.length,
          },
        };
        console.log(JSON.stringify(output));
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });

  forumsCmd
    .command("posts")
    .description("Show posts in a discussion")
    .argument("<discussion-id>", "Discussion ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (discussionId, options, command) => {
      const context = await createSessionContext(options, command);
      if (!context) {
        process.exitCode = 1;
        return;
      }

      const { log, page, session, browser, context: browserContext } = context;
      const output: OutputFormat = getOutputFormat(command);

      try {
        const posts = await getDiscussionPosts(page, session, parseInt(discussionId, 10));

        if (output === "json") {
          const result = {
            status: "success",
            timestamp: new Date().toISOString(),
            discussion_id: discussionId,
            posts: posts.map(p => ({
              id: p.id,
              subject: p.subject,
              author: p.author,
              author_id: p.authorId,
              created: new Date(p.created * 1000).toISOString(),
              modified: new Date(p.modified * 1000).toISOString(),
              message: p.message,
              unread: p.unread,
            })),
            summary: {
              total_posts: posts.length,
            },
          };
          console.log(JSON.stringify(result));
        } else if (output === "table") {
          console.log(`Discussion ${discussionId} - ${posts.length} posts`);
          console.log("Use --output json to see full post content");
          const tablePosts = posts.map(p => ({
            id: p.id,
            subject: p.subject.substring(0, 50) + (p.subject.length > 50 ? "..." : ""),
            author: p.author,
            created: new Date(p.created * 1000).toLocaleString(),
          }));
          formatAndOutput(tablePosts as unknown as Record<string, unknown>[], "table", log);
        }
      } finally {
        await closeBrowserSafely(browser, browserContext);
      }
    });
}
