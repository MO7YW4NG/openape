import { getBaseDir, stripHtmlTags, getOutputFormat } from "../lib/utils.ts";
import { Command } from "commander";
import type { Logger, OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi, getForumsApi, getForumDiscussionsApi, getDiscussionPostsApi, addForumDiscussionApi, addForumPostApi } from "../lib/moodle.ts";
import { createLogger } from "../lib/logger.ts";
import { loadWsToken, loadSesskey } from "../lib/token.ts";
import path from "node:path";
import fs from "node:fs";

interface ForumWithCourse {
  course_id: number;
  course_name: string;
  cmid: string;
  forum_id: number;
  name: string;
  timemodified: number;
  // url: string;
}

export function registerForumsCommand(program: Command): void {
  const forumsCmd = program.command("forums");
  forumsCmd.description("Forum operations");

  // Pure API context - no browser required (fast!)
  async function createApiContext(options: { verbose?: boolean; headed?: boolean }, command?: any): Promise<{
    log: Logger;
    session: { wsToken: string; moodleBaseUrl: string; sesskey?: string };
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

    // Try to load sesskey from cache
    const sesskey = loadSesskey(sessionPath) || undefined;

    return {
      log,
      session: {
        wsToken,
        moodleBaseUrl: "https://ilearning.cycu.edu.tw",
        sesskey,
      },
    };
  }

  forumsCmd
    .command("list")
    .description("List forums from in-progress courses")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

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
            timemodified: wsForum.timemodified,
            // url: `https://ilearning.cycu.edu.tw/mod/forum/view.php?id=${wsForum.cmid}`,
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
    });

  forumsCmd
    .command("list-all")
    .description("List all forums across all courses")
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
            timemodified: wsForum.timemodified,
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
    });

  forumsCmd
    .command("discussions")
    .description("List discussions in a forum (use forum ID)")
    .argument("<forum-id>", "Forum ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (forumId, options, command) => {
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      // Get courses via WS API
      const courses = await getEnrolledCoursesApi(apiContext.session, {
        classification: "inprogress",
      });

      // Get forums via WS API
      const courseIds = courses.map(c => c.id);
      const wsForums = await getForumsApi(apiContext.session, courseIds);

      // Find forum by cmid or instance ID
      const targetForum = wsForums.find(
        f => f.cmid.toString() === forumId || f.id === parseInt(forumId, 10)
      );

      if (!targetForum) {
        console.log(JSON.stringify({ status: "error", error: "Forum not found" }));
        process.exitCode = 1;
        return;
      }

      const course = courses.find(c => c.id === targetForum.courseid);

      // Get discussions via WS API
      const discussions = await getForumDiscussionsApi(apiContext.session, targetForum.id);

      const result = {
        status: "success",
        timestamp: new Date().toISOString(),
        forum_id: targetForum.id,
        forum_name: targetForum.name,
        course_id: course?.id,
        course_name: course?.fullname,
        discussions: discussions.map(d => ({
          id: d.id,
          name: d.name,
          user_id: d.userId,
          time_modified: d.timeModified,
          post_count: d.postCount,
          unread: d.unread,
          message: (stripHtmlTags(d.message || "")).substring(0, 250) + "...",
        })),
        summary: {
          total_discussions: discussions.length,
        },
      };
      console.log(JSON.stringify(result));
    });

  forumsCmd
    .command("posts")
    .description("Show posts in a discussion")
    .argument("<discussion-id>", "Discussion ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (discussionId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const posts = await getDiscussionPostsApi(apiContext.session, parseInt(discussionId, 10));

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
        console.table(tablePosts);
      }
    });

  forumsCmd
    .command("post")
    .description("Post a new discussion to a forum")
    .argument("<forum-id>", "Forum ID")
    .argument("<subject>", "Discussion subject")
    .argument("<message>", "Discussion message")
    .option("--subscribe", "Subscribe to the discussion", false)
    .option("--pin", "Pin the discussion", false)
    .action(async (forumId, subject, message, options, command) => {
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const { log, session } = apiContext;

      // Get courses to find the forum
      const courses = await getEnrolledCoursesApi(session, {
        classification: "inprogress",
      });

      const courseIds = courses.map(c => c.id);
      const wsForums = await getForumsApi(session, courseIds);

      // Find forum by cmid or instance ID
      const targetForum = wsForums.find(
        f => f.cmid.toString() === forumId || f.id === parseInt(forumId, 10)
      );

      if (!targetForum) {
        log.error(`Forum not found: ${forumId}`);
        process.exitCode = 1;
        return;
      }

      const course = courses.find(c => c.id === targetForum.courseid);
      log.info(`Posting to forum: ${targetForum.name} (${course?.fullname})`);

      const result = await addForumDiscussionApi(
        session,
        targetForum.id,
        subject,
        message,
        { subscribe: options.subscribe, pin: options.pin }
      );

      if (result.success) {
        log.success(`✓ Discussion posted successfully!`);
        log.info(`  Discussion ID: ${result.discussionId}`);
      } else {
        log.error(`✗ Failed to post discussion: ${result.error}`);
        process.exitCode = 1;
      }
    });

  forumsCmd
    .command("reply")
    .description("Reply to a discussion post")
    .argument("<post-id>", "Parent post ID to reply to")
    .argument("<subject>", "Reply subject")
    .argument("<message>", "Reply message")
    .option("--attachment-id <id>", "Draft file ID for attachment")
    .option("--inline-attachment-id <id>", "Draft file ID for inline attachment")
    .action(async (postId, subject, message, options, command) => {
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const { log, session } = apiContext;

      log.info(`Replying to post: ${postId}`);
      log.info(`  Subject: ${subject}`);
      log.info(`  Message: ${message}`);
      if (options.attachmentId) {
        log.info(`  Attachment ID: ${options.attachmentId}`);
      }

      const result = await addForumPostApi(
        session,
        parseInt(postId, 10),
        subject,
        message,
        {
          attachmentId: options.attachmentId ? parseInt(options.attachmentId, 10) : undefined,
          inlineAttachmentId: options.inlineAttachmentId ? parseInt(options.inlineAttachmentId, 10) : undefined,
        }
      );

      if (result.success) {
        log.success(`✓ Reply posted successfully!`);
        log.info(`  Post ID: ${result.postId}`);
      } else {
        log.error(`✗ Failed to post reply: ${result.error}`);
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
