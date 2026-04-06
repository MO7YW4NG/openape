import { getOutputFormat, formatTimestamp } from "../lib/utils.ts";
import { Command } from "commander";
import type { OutputFormat } from "../lib/types.ts";
import { getSiteInfoApi, getMessagesApi, getDiscussionPostsApi } from "../lib/moodle.ts";
import { createApiContext } from "../lib/auth.ts";
import { formatAndOutput } from "../index.ts";

export function registerAnnouncementsCommand(program: Command): void {
  const announcementsCmd = program.command("announcements");
  announcementsCmd.description("Announcement operations");

  announcementsCmd
    .command("list-all")
    .description("List all announcements across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .option("--unread-only", "Show only unread announcements")
    .option("--limit <n>", "Maximum number of announcements to show", "20")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const limit = parseInt(options.limit, 10);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const siteInfo = await getSiteInfoApi(apiContext.session);

      const messages = await getMessagesApi(apiContext.session, siteInfo.userid, {
        limitnum: limit,
      });

      const allAnnouncements = messages.map(m => ({
        course_id: 0,
        course_name: "Notifications",
        id: m.id,
        subject: m.subject,
        author: `User ${m.useridfrom}`,
        author_id: m.useridfrom,
        created_at: formatTimestamp(m.timecreated),
        modified_at: formatTimestamp(m.timecreated),
        unread: false,
      }));

      allAnnouncements.sort((a, b) => (b as any).created_at > (a as any).created_at ? 1 : -1);

      const shown = allAnnouncements.slice(0, limit);

      formatAndOutput(
        shown as unknown as Record<string, unknown>[],
        output,
        apiContext.log,
        { status: "success", timestamp: new Date().toISOString(), total_announcements: allAnnouncements.length, shown: shown.length }
      );
    });

  announcementsCmd
    .command("read")
    .description("Read a specific announcement (shows full content)")
    .argument("<announcement-id>", "Discussion ID of the announcement")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (announcementId, options, command) => {
      const output: OutputFormat = getOutputFormat(command);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

      const posts = await getDiscussionPostsApi(apiContext.session, parseInt(announcementId, 10));

      if (posts.length === 0) {
        apiContext.log.error(`Announcement not found: ${announcementId}`);
        process.exitCode = 1;
        return;
      }

      const firstPost = posts[0];

      formatAndOutput(
        {
          id: announcementId,
          subject: firstPost.subject,
          author: firstPost.author,
          author_id: firstPost.authorId,
          created_at: formatTimestamp(firstPost.created),
          modified_at: formatTimestamp(firstPost.modified),
          message: firstPost.message,
        } as unknown as Record<string, unknown>,
        output,
        apiContext.log
      );
    });
}
