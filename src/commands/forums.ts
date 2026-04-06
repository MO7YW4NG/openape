import { stripHtmlTags, getOutputFormat, formatTimestamp } from "../lib/utils.ts";
import { Command } from "commander";
import type { OutputFormat } from "../lib/types.ts";
import { getEnrolledCoursesApi, getForumsApi, getForumDiscussionsApi, getDiscussionPostsApi, addForumDiscussionApi, addForumPostApi, deleteForumPostApi, resolveForumId } from "../lib/moodle.ts";
import { createApiContext } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";

interface ForumWithCourse {
  course_id: number;
  course_name: string;
  cmid: string;
  forum_id: number;
  name: string;
  intro: string;
  timemodified: number;
  // url: string;
}

export function registerForumsCommand(program: Command): void {
  const forumsCmd = program.command("forums");
  forumsCmd.description("Forum operations");

  async function listForums(classification?: "inprogress" | "past" | "future" | "all") {
    const apiContext = await createApiContext({});
    if (!apiContext) {
      process.exitCode = 1;
      return;
    }

    const courses = await getEnrolledCoursesApi(apiContext.session, {
      classification,
    });

    const courseIds = courses.map(c => c.id);
    const wsForums = await getForumsApi(apiContext.session, courseIds);

    const courseMap = new Map(courses.map(c => [c.id, c]));
    const allForums: ForumWithCourse[] = [];
    for (const wsForum of wsForums) {
      const course = courseMap.get(wsForum.courseid);
      if (course) {
        allForums.push({
          course_id: wsForum.courseid,
          course_name: course.fullname,
          intro: wsForum.intro,
          cmid: wsForum.cmid.toString(),
          forum_id: wsForum.id,
          name: wsForum.name,
          timemodified: wsForum.timemodified,
        });
      }
    }

    formatAndOutput(
      allForums as unknown as Record<string, unknown>[],
      "json",
      apiContext.log,
      { status: "success", timestamp: new Date().toISOString(), total_courses: courses.length, total_forums: allForums.length }
    );
  }

  forumsCmd
    .command("list")
    .description("List forums from in-progress courses")
    .action(() => listForums("inprogress"));

  forumsCmd
    .command("list-all")
    .description("List all forums across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .action(async (options) => {
      const classification = options.level === "all" ? undefined : "inprogress";
      await listForums(classification);
    });

  forumsCmd
    .command("discussions")
    .description("List discussions in a forum (use forum ID)")
    .argument("<forum-id>", "Forum ID")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (forumId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const resolved = await resolveForumId(apiContext.session, forumId);
      if (!resolved) {
        apiContext.log.error("Forum not found");
        process.exitCode = 1;
        return;
      }

      const discussions = await getForumDiscussionsApi(apiContext.session, resolved.forumId);

      const items = discussions.map(d => ({
        id: d.id,
        name: d.name,
        user_id: d.userId,
        time_modified: d.timeModified,
        post_count: d.postCount,
        unread: d.unread,
        message: stripHtmlTags(d.message || ""),
      }));

      formatAndOutput(
        items as unknown as Record<string, unknown>[],
        output,
        apiContext.log,
        { status: "success", timestamp: new Date().toISOString(), forum_id: resolved.forumId, forum_name: resolved.name ?? null, course_id: resolved.courseid ?? null, total_discussions: discussions.length }
      );
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

      const items = posts.map(p => ({
        id: p.id,
        subject: p.subject,
        author: p.author,
        author_id: p.authorId,
        created: formatTimestamp(p.created),
        modified: formatTimestamp(p.modified),
        message: p.message,
        unread: p.unread,
      }));

      formatAndOutput(
        items as unknown as Record<string, unknown>[],
        output,
        apiContext.log,
        { status: "success", timestamp: new Date().toISOString(), discussion_id: discussionId, total_posts: posts.length }
      );
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

      const resolved = await resolveForumId(session, forumId);
      if (!resolved) {
        log.error(`Forum not found: ${forumId}`);
        process.exitCode = 1;
        return;
      }

      log.info(`Posting to forum: ${resolved.name ?? forumId}`);

      const result = await addForumDiscussionApi(
        session,
        resolved.forumId,
        subject,
        message
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

  forumsCmd
    .command("delete")
    .description("Delete a forum post or discussion (by post ID)")
    .argument("<post-id>", "Post ID to delete (deletes entire discussion if it's the first post)")
    .action(async (postId, options, command) => {
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const { log, session } = apiContext;

      const result = await deleteForumPostApi(session, parseInt(postId, 10));

      if (result.success) {
        log.success(`✓ Post ${postId} deleted successfully!`);
      } else {
        log.error(`✗ Failed to delete post: ${result.error}`);
        process.exitCode = 1;
      }
    });
}