import { formatTimestamp } from "../lib/utils.ts";
import { Command } from "commander";
import { getSiteInfoApi, getMessagesApi, getDiscussionPostsApi } from "../lib/moodle.ts";
import { createApiContext } from "../lib/auth.ts";

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

  announcementsCmd
    .command("list-all")
    .description("List all announcements across all courses")
    .option("--level <type>", "Course level: in_progress (default) | all", "in_progress")
    .option("--unread-only", "Show only unread announcements")
    .option("--limit <n>", "Maximum number of announcements to show", "20")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (options, command) => {
      const limit = parseInt(options.limit, 10);
      const apiContext = await createApiContext(options, command);
      if (!apiContext) {
        process.exitCode = 1;
        return;
      }

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

      console.log(JSON.stringify({
        status: "success",
        timestamp: new Date().toISOString(),
        level: options.level,
        total_announcements: allAnnouncements.length,
        shown: filteredAnnouncements.length,
      }));
      for (const a of filteredAnnouncements) {
        console.log(JSON.stringify({
          course_id: a.course_id,
          course_name: a.course_name,
          id: a.id,
          subject: a.subject,
          author: a.author,
          author_id: a.authorId,
          created_at: formatTimestamp(a.createdAt),
          modified_at: formatTimestamp(a.modifiedAt),
          unread: a.unread,
        }));
      }
    });

  announcementsCmd
    .command("read")
    .description("Read a specific announcement (shows full content)")
    .argument("<announcement-id>", "Discussion ID of the announcement")
    .option("--output <format>", "Output format: json|csv|table|silent")
    .action(async (announcementId, options, command) => {
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

      const output = {
        status: "success",
        timestamp: new Date().toISOString(),
        announcement: {
          id: announcementId,
          subject: firstPost.subject,
          author: firstPost.author,
          author_id: firstPost.authorId,
          created_at: formatTimestamp(firstPost.created),
          modified_at: formatTimestamp(firstPost.modified),
          message: firstPost.message,
        },
      };
      console.log(JSON.stringify(output));
    });
}
