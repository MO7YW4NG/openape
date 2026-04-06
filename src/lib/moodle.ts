import type { Page } from "playwright-core";
import { stripHtmlTags, extractCourseName } from "./utils.ts";
import * as fs from "node:fs";
import type {
  SessionInfo,
  Logger,
  EnrolledCourse,
  SuperVideoModule,
  QuizModule,
  QuizAttempt,
  QuizAttemptData,
  QuizQuestion,
  QuizStartResult,
  ResourceModule,
  ForumDiscussion,
  ForumPost,
  CalendarEvent,
  CourseGrade,
} from "./types.ts";

// ── Core Moodle AJAX Wrapper ───────────────────────────────────────────

/**
 * Moodle WS API functions that are known to work via /webservice/rest/server.php
 * Other functions should use the sesskey-based AJAX API.
 */
const WS_API_FUNCTIONS = new Set([
  "mod_forum_get_forums_by_courses",
  "mod_forum_get_forum_discussions",
  "mod_forum_get_forum_discussion_posts",
  "mod_forum_add_discussion",
  "mod_forum_add_discussion_post",
  "mod_forum_delete_post",
  "core_files_upload",
  "core_files_get_files",
  "core_files_get_unused_draft_itemid",
  "gradereport_user_get_grade_items",
  "core_calendar_get_calendar_events",
  "core_course_get_contents",
  "core_course_get_course_module",
  "core_completion_get_activities_completion_status",
  "core_completion_update_activity_completion_status_manually",
  "mod_supervideo_progress_save",
  "mod_supervideo_progress_save_mobile",
  "mod_supervideo_view_supervideo",
  "mod_quiz_get_quizzes_by_courses",
  "mod_quiz_start_attempt",
  "mod_quiz_get_attempt_data",
  "mod_resource_get_resources_by_courses",
  "mod_assign_get_assignments",
  "mod_assign_save_submission",
  "mod_assign_get_submission_status",
  "core_message_get_messages",
  "core_webservice_get_site_info",
]);

/**
 * Convert args to URLSearchParams, handling arrays properly for Moodle WS API.
 * Moodle expects array parameters as: courseids[0]=1&courseids[1]=2
 * For options array: options[0][name]=attachmentsid&options[0][value]=123
 */
function buildWsParams(args: Record<string, unknown>): URLSearchParams {
  const params = new URLSearchParams();

  for (const [key, value] of Object.entries(args)) {
    if (key === "options" && Array.isArray(value)) {
      // Special handling for options array: options[0][name]=xxx&options[0][value]=yyy
      value.forEach((opt: any, i) => {
        if (opt && typeof opt === "object" && "name" in opt && "value" in opt) {
          params.append(`${key}[${i}][name]`, String(opt.name));
          params.append(`${key}[${i}][value]`, String(opt.value));
        }
      });
    } else if (Array.isArray(value)) {
      // Array parameters: courseids[0]=1&courseids[1]=2
      value.forEach((v, i) => {
        params.append(`${key}[${i}]`, String(v));
      });
    } else if (value !== null && value !== undefined) {
      params.append(key, String(value));
    }
  }

  return params;
}

/**
 * Direct HTTP API call without browser (for WS API only).
 * This is much faster than browser-based calls.
 */
export async function moodleApiCall<T = unknown>(
  session: { wsToken: string; moodleBaseUrl: string },
  methodname: string,
  args: Record<string, unknown>
): Promise<T> {
  if (!session.wsToken) {
    throw new Error(`WS ${methodname} required for API call: ${methodname}`);
  }

  const params = buildWsParams(args);
  params.set("wstoken", session.wsToken);
  params.set("wsfunction", methodname);
  params.set("moodlewsrestformat", "json");

  const url = `${session.moodleBaseUrl}/webservice/rest/server.php?${params.toString()}`;

  const response = await fetch(url, { method: "GET" });
  const result = await response.json();

  if (result.error) {
    throw new Error(
      `WS ${methodname} failed: ${result.message ?? result.exception?.message ?? "Unknown error"}`
    );
  }

  return result as T;
}

/**
 * Send a Moodle AJAX request and return the result.
 * Uses Web Service token if available AND the function is in WS_API_FUNCTIONS,
 * otherwise falls back to sesskey-based AJAX (via /lib/ajax/service.php).
 */
export async function moodleAjax<T = unknown>(
  page: Page,
  session: SessionInfo,
  methodname: string,
  args: Record<string, unknown>
): Promise<T> {
  // Only use WS API for known WS functions
  const useWsApi = session.wsToken && WS_API_FUNCTIONS.has(methodname);

  if (useWsApi) {
    // Use Moodle Web Service API
    // Format: /webservice/rest/server.php?wstoken=TOKEN&wsfunction=FUNCTION&moodlewsrestformat=json
    const params = buildWsParams(args);
    params.set("wstoken", session.wsToken!);
    params.set("wsfunction", methodname);
    params.set("moodlewsrestformat", "json");

    const url = `${session.moodleBaseUrl}/webservice/rest/server.php?${params.toString()}`;

    const result = await page.evaluate(
      async ({ url }) => {
        const res = await fetch(url, { method: "GET" });
        return res.json();
      },
      { url }
    );

    if (result.error) {
      throw new Error(
        `WS ${methodname} failed: ${result.message ?? result.exception?.message ?? "Unknown error"}`
      );
    }

    return result as T;
  } else {
    // Legacy sesskey-based AJAX format
    const url = `${session.moodleBaseUrl}/lib/ajax/service.php?sesskey=${session.sesskey!}&info=${methodname}`;
    const payload = [{ index: 0, methodname, args }];

    const result = await page.evaluate(
      async ({ url, payload }) => {
        const res = await fetch(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });
        return res.json();
      },
      { url, payload }
    );

    if (result?.[0]?.error) {
      throw new Error(
        `AJAX ${methodname} failed: ${result[0].exception?.message ?? "Unknown error"}`
      );
    }

    return result[0].data as T;
  }
}

// ── Course Operations ─────────────────────────────────────────────────────

/**
 * Fetch enrolled courses via pure API (no browser required).
 * Fast and lightweight - uses HTTP fetch directly.
 */
export async function getEnrolledCoursesApi(
  session: { wsToken: string; moodleBaseUrl: string },
  options: { classification?: "inprogress" | "past" | "future" | "all"; limit?: number } = {}
): Promise<EnrolledCourse[]> {
  const data = await moodleApiCall<{ courses?: unknown[] }>(
    session,
    "core_course_get_enrolled_courses_by_timeline_classification",
    {
      offset: 0,
      limit: options.limit ?? 0,
      classification: options.classification ?? "all",
      sort: "fullname",
      customfieldname: "",
      customfieldvalue: "",
      requiredfields: [
        "id",
        "fullname",
        "shortname",
        "idnumber",
        "category",
        "progress",
        "startdate",
        "enddate",
      ],
    }
  );

  return (data?.courses ?? []).map((c: any) => ({
    id: c.id,
    fullname: extractCourseName(c.fullname),
    shortname: c.shortname,
    idnumber: c.idnumber,
    category: c.category?.name,
    progress: c.progress,
    startdate: c.startdate,
    enddate: c.enddate,
  }));
}

/**
 * Fetch all enrolled courses via Moodle AJAX API.
 */
export async function getEnrolledCourses(
  page: Page,
  session: SessionInfo,
  log: Logger,
  options: { classification?: "inprogress" | "past" | "future"; limit?: number } = {}
): Promise<EnrolledCourse[]> {
  log.debug("Fetching enrolled courses via AJAX...");

  const data = await moodleAjax<{ courses?: unknown[] }>(
    page,
    session,
    "core_course_get_enrolled_courses_by_timeline_classification",
    {
      offset: 0,
      limit: options.limit ?? 0,
      classification: options.classification ?? "all",
      sort: "fullname",
      customfieldname: "",
      customfieldvalue: "",
      requiredfields: [
        "id",
        "fullname",
        "shortname",
        "showcoursecategory",
        "showshortname",
        "visible",
        "enddate",
      ],
    }
  );

  const courses: EnrolledCourse[] = (data?.courses ?? []).map((c: any) => ({
    id: c.id,
    fullname: extractCourseName(c.fullname),
    shortname: c.shortname,
    idnumber: c.idnumber,
    category: c.category?.name,
    progress: c.progress,
    startdate: c.startdate,
    enddate: c.enddate,
  }));

  log.debug(`Found ${courses.length} course${courses.length === 1 ? "" : "s"}.`);
  return courses;
}

/**
 * Get course state (modules) via core_courseformat_get_state.
 */
export async function getCourseState(
  page: Page,
  session: SessionInfo,
  courseId: number
): Promise<any> {
  const data = await moodleAjax<string>(
    page,
    session,
    "core_courseformat_get_state",
    {
      courseid: courseId,
    }
  );

  return typeof data === "string" ? JSON.parse(data) : data;
}

// ── Video Operations ──────────────────────────────────────────────────────

/**
 * Get all SuperVideo modules in a course.
 */
export async function getSupervideosInCourse(
  page: Page,
  session: SessionInfo,
  courseId: number,
  log: Logger,
  options: { incompleteOnly?: boolean } = {}
): Promise<SuperVideoModule[]> {
  const state = await getCourseState(page, session, courseId);
  const cms: any[] = state?.cm ?? [];

  const allSupervideos = cms.filter((cm: any) => cm.module === "supervideo" || cm.modname === "supervideo");

  // Filter: Only include videos with completion tracking enabled (have completionstate field)
  // and are not yet completed (completionstate != 1 or isoverallcomplete != true)
  const incomplete = allSupervideos.filter((cm: any) => {
    // Has completionstate field = completion tracking is enabled for this video
    const hasCompletionTracking = "completionstate" in cm;
    // Is not yet completed
    const isIncomplete = cm.completionstate !== 1 && cm.isoverallcomplete !== true;

    return hasCompletionTracking && isIncomplete;
  });

  log.debug(
    `  SuperVideo: ${allSupervideos.length} total, ${incomplete.length} incomplete (with completion enabled)`
  );

  // Return only incomplete if requested, otherwise return all
  const videos = options.incompleteOnly ? incomplete : allSupervideos;

  return videos.map((cm: any) => ({
    cmid: cm.cmid?.toString() ?? cm.id?.toString() ?? "",
    name: cm.name,
    url: cm.url,
    isComplete: !!cm.isoverallcomplete,
  }));
}

// ── Forum Operations ──────────────────────────────────────────────────────

/**
 * Get all forums via pure WS API (no browser required).
 * Fast and lightweight - uses HTTP fetch directly.
 */
export async function getForumsApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseIds: number[]
): Promise<Array<{ id: number; cmid: number; name: string; intro: string; courseid: number; timemodified: number }>> {
  const data = await moodleApiCall<any[]>(
    session,
    "mod_forum_get_forums_by_courses",
    { courseids: courseIds }
  );

  return (data ?? []).map((f: any) => ({
    id: f.id,
    cmid: f.cmid,
    name: f.name,
    intro: f.intro,
    courseid: f.course,  // API returns 'course' not 'courseid'
    timemodified: f.timemodified,
  }));
}

/**
 * Resolve a forum ID (cmid or instance ID) to a forum instance ID.
 * Tries cmid resolution first (via core_course_get_course_module) to get name/course info.
 * Falls back to treating the ID as a raw forum instance ID.
 */
export async function resolveForumId(
  session: { wsToken: string; moodleBaseUrl: string },
  id: string
): Promise<{ forumId: number; cmid?: number; name?: string; courseid?: number } | null> {
  const numId = parseInt(id, 10);

  // Try cmid resolution first (gets name + course info)
  try {
    const cm = await moodleApiCall<{ cm: { instance: number; name: string; course: number; modname: string } }>(
      session,
      "core_course_get_course_module",
      { cmid: numId }
    );
    if (cm?.cm && cm.cm.modname === "forum") {
      return {
        forumId: cm.cm.instance,
        cmid: numId,
        name: cm.cm.name,
        courseid: cm.cm.course,
      };
    }
  } catch {
    // Not a valid cmid, try as forum instance ID
  }

  // Fall back: treat as forum instance ID directly
  try {
    const data = await moodleApiCall<{ discussions?: unknown[] }>(
      session,
      "mod_forum_get_forum_discussions",
      { forumid: numId, limit: 1 }
    );
    // If we get discussions back (even empty), the forum exists
    if (data) {
      return { forumId: numId };
    }
  } catch {
    // Invalid forum instance ID
  }

  return null;
}

/**
 * Get discussions in a forum via WS API (no browser required).
 * Uses mod_forum_get_forum_discussions
 */
export async function getForumDiscussionsApi(
  session: { wsToken: string; moodleBaseUrl: string },
  forumId: number,
  options?: {
    sortorder?: number; // 1=oldest first, 2=newest first, 3=most recently modified
    page?: number;
    perpage?: number;
    groupid?: number;
  }
): Promise<ForumDiscussion[]> {
  const params: Record<string, number> = { forumid: forumId, sortorder: options?.sortorder ?? 2 };
  if (options?.page !== undefined) params.page = options.page;
  if (options?.perpage !== undefined) params.perpage = options.perpage;
  if (options?.groupid !== undefined) params.groupid = options.groupid;

  const data = await moodleApiCall<{ discussions?: unknown[] }>(
    session,
    "mod_forum_get_forum_discussions",
    params
  );

  return (data?.discussions ?? []).map((d: any) => ({
    id: d.discussion,
    forumId: d.forum,
    name: d.name,
    firstPostId: d.firstpost,
    userId: d.userid,
    userFullName: d.userfullname || "",
    groupId: d.groupid,
    timedue: d.timedue,
    timeModified: d.timemodified,
    timeStart: d.timestart,
    timeEnd: d.timeend,
    userModified: d.usermodified,
    userModifiedFullName: d.usermodifiedfullname,
    postCount: d.numreplies,
    unread: (d.numunread ?? 0) > 0,
    subject: stripHtmlTags(d.subject ?? ""),
    message: d.message,
    pinned: d.pinned,
    locked: d.locked,
    starred: d.starred,
  }));
}

/**
 * Get posts in a discussion via WS API (no browser required).
 * Uses mod_forum_get_forum_discussion_posts
 */
export async function getDiscussionPostsApi(
  session: { wsToken: string; moodleBaseUrl: string },
  discussionId: number
): Promise<ForumPost[]> {
  try {
    const data = await moodleApiCall<{ posts?: unknown[] }>(
      session,
      "mod_forum_get_discussion_posts",
      {
        discussionid: discussionId,
      }
    );

    if (!data?.posts || data.posts.length === 0) {
      return [];
    }

    return (data.posts as any[]).map((p: any) => ({
      id: p.id,
      subject: stripHtmlTags(p.subject || ""),
      author: p.author?.fullname ?? "Unknown",
      authorId: p.author?.id ?? p.userid,
      created: p.timecreated,
      modified: p.timemodified,
      message: stripHtmlTags(p.message || ""),
      discussionId: p.discussionid,
      unread: p.unread ?? false,
    }));
  } catch (error) {
    // Return empty array on error instead of throwing
    // This allows commands to gracefully handle inaccessible discussions
    return [];
  }
}

/**
 * Delete a forum post. If the post is a discussion's topic post,
 * the entire discussion is deleted.
 */
export async function deleteForumPostApi(
  session: { wsToken: string; moodleBaseUrl: string },
  postId: number,
): Promise<{ success: boolean; error?: string }> {
  try {
    const data = await moodleApiCall<{ status: boolean; warnings: unknown[] }>(
      session,
      "mod_forum_delete_post",
      { postid: postId }
    );
    return { success: data?.status === true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

/**
 * Add a new discussion to a forum.
 */
export async function addForumDiscussionApi(
  session: { wsToken: string; moodleBaseUrl: string },
  forumId: number,
  subject: string,
  message: string,
): Promise<{ success: boolean; discussionId?: number; error?: string }> {
  try {
    const data = await moodleApiCall<any>(
      session,
      "mod_forum_add_discussion",
      {
        forumid: forumId,
        subject,
        message: message.replace(/\n/g, "<br>"),
      }
    );

    if (data?.discussionid) {
      return { success: true, discussionId: data.discussionid };
    }

    return {
      success: false,
      error: data?.message ?? data?.error ?? "Failed to add discussion",
    };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

/**
 * Add a reply post to a discussion.
 */
export async function addForumPostApi(
  session: { wsToken: string; moodleBaseUrl: string },
  postId: number, // Parent post ID to reply to
  subject: string,
  message: string,
  options?: {
    inlineAttachmentId?: number;
    attachmentId?: number;
  }
): Promise<{ success: boolean; postId?: number; error?: string }> {
  try {
    // Build options array for Moodle WS API
    const apiOptions: Array<{ name: string; value: string | number }> = [];

    if (options?.inlineAttachmentId !== undefined) {
      apiOptions.push({ name: "inlineattachmentsid", value: options.inlineAttachmentId });
    }
    if (options?.attachmentId !== undefined) {
      apiOptions.push({ name: "attachmentsid", value: options.attachmentId });
    }

    const params: Record<string, unknown> = {
      postid: postId,
      subject,
      message: message.replace(/\n/g, "<br>"),
      messageformat: 1, // 1 = HTML, 0 = Moodle, 2 = Plain text, 3 = Markdown
    };

    // Only add options if not empty
    if (apiOptions.length > 0) {
      params.options = apiOptions;
    }

    console.debug(`[DEBUG] add_discussion_post params:`, JSON.stringify(params, null, 2));

    const data = await moodleApiCall<any>(
      session,
      "mod_forum_add_discussion_post",
      params
    );

    if (data?.postid) {
      return { success: true, postId: data.postid };
    }

    return {
      success: false,
      error: data?.message ?? data?.error ?? "Failed to add post",
    };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

// ── Resource/Material Operations ──────────────────────────────────────────

/**
 * Get all resource modules in a course.
 */
export async function getResourcesInCourse(
  page: Page,
  session: SessionInfo,
  courseId: number,
  log: Logger
): Promise<ResourceModule[]> {
  const state = await getCourseState(page, session, courseId);
  const cms: any[] = state?.cm ?? [];

  const resources = cms.filter((cm: any) =>
    ["resource", "url"].includes(cm.module)
  );

  log.debug(`  Found ${resources.length} resource${resources.length === 1 ? "" : "s"}.`);

  return resources.map((cm: any) => ({
    cmid: cm.cmid?.toString() ?? cm.id?.toString() ?? "",
    name: cm.name,
    url: cm.url,
    courseId,
    modType: cm.module,
    mimetype: undefined,
    filesize: undefined,
    modified: 0,
  }));
}

// ── Calendar Operations ─────────────────────────────────────────────────────

/**
 * Get calendar events via pure WS API (no browser required).
 * Fast and lightweight - uses HTTP fetch directly.
 */
export async function getCalendarEventsApi(
  session: { wsToken: string; moodleBaseUrl: string },
  options: {
    courseId?: number;
    startTime?: number;
    endTime?: number;
    events?: { courseid?: number; groupid?: number; categoryid?: number }[];
  } = {}
): Promise<CalendarEvent[]> {
  const data = await moodleApiCall<{ events?: unknown[] }>(
    session,
    "core_calendar_get_calendar_events",
    {
      ...options,
    }
  );

  return (data?.events ?? []).map((e: any) => ({
    id: e.id,
    name: e.name,
    description: e.description,
    format: e.format,
    courseid: e.courseid,
    categoryid: e.categoryid,
    groupid: e.groupid,
    userid: e.userid,
    moduleid: e.moduleid,
    modulename: e.modulename,
    instance: e.instance,
    eventtype: e.eventtype,
    timestart: e.timestart * 1000, // Convert to milliseconds
    timeduration: e.timeduration ? e.timeduration * 1000 : undefined,
    timedue: e.timedue ? e.timedue * 1000 : undefined,
    visible: e.visible,
    location: e.location,
  }));
}

// ── Grade Operations ──────────────────────────────────────────────────────

/**
 * Get course grades for the current user via pure WS API (no browser required).
 * Fast and lightweight - uses HTTP fetch directly.
 */
export async function getCourseGradesApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseId: number
): Promise<CourseGrade> {
  const data = await moodleApiCall<{ usergrades?: unknown[] }>(
    session,
    "gradereport_user_get_grade_items",
    { courseid: courseId }
  );

  // The API returns grade items for the course
  const gradeItems = (data?.usergrades ?? []) as any[];

  // Return a single CourseGrade object with items array
  return {
    courseId,
    courseName: gradeItems[0]?.coursefullname ?? "",
    grade: gradeItems[0]?.grade,
    gradeFormatted: gradeItems[0]?.gradeformatted,
    rank: gradeItems[0]?.rank,
    totalUsers: gradeItems[0]?.totalusers,
    items: gradeItems.map((g: any) => ({
      id: g.id,
      name: g.itemname || g.itemtype,
      grade: g.grade,
      gradeFormatted: g.gradeformatted,
      range: g.grade ? `${g.grademin ?? 0}-${g.grademax ?? 100}` : undefined,
    })),
  };
}

// ── Video Metadata (from original course.ts) ───────────────────────────────

/**
 * Visit a SuperVideo activity page and extract view_id + duration.
 */
/**
 * Optimized video metadata extraction - minimal page load overhead.
 * Blocks images, fonts, stylesheets to speed up viewId extraction.
 */
export async function getVideoMetadata(
  page: Page,
  activityUrl: string,
  log: Logger
): Promise<{ name: string; url: string; viewId: number; duration: number; existingPercent: number; videoSources: string[]; youtubeIds?: string[] }> {
  // Block unnecessary resources for faster loading
  await page.route("**/*.{png,jpg,jpeg,gif,webp,svg,ico,woff,woff2,ttf,css}", (route) => route.abort());

  await page.goto(activityUrl, { waitUntil: "domcontentloaded", timeout: 20000 });

  const name = await page.title();
  const pageSource = await page.content();

  let viewId: number | null = null;
  const viewIdPatterns = [
    /player_create.*?amd\.\w+\((\d+)/,
    /view_id['":\s]+(\d+)/,
  ];
  for (const pattern of viewIdPatterns) {
    const match = pageSource.match(pattern);
    if (match) {
      viewId = parseInt(match[1], 10);
      break;
    }
  }

  if (viewId === null) {
    throw new Error(`Could not extract view_id from ${activityUrl}`);
  }

  let duration: number | null = null;
  const isYoutube = pageSource.includes("youtube.com") || pageSource.includes("youtu.be");

  if (!isYoutube) {
    try {
      await page.waitForSelector("video", { timeout: 10000 });
      duration = await page.evaluate(() => {
        return new Promise<number | null>((resolve) => {
          const media = document.querySelector("video") as HTMLMediaElement | null;
          if (!media) return resolve(null);
          if (media.duration && isFinite(media.duration)) {
            return resolve(Math.ceil(media.duration));
          }
          media.addEventListener("loadedmetadata", () => {
            resolve(Math.ceil(media.duration));
          });
          setTimeout(() => resolve(null), 8000);
        });
      });
    } catch {
      // no video element
    }
  }

  if (!duration) {
    const durationMatch = pageSource.match(/["']?duration["']?\s*[:=]\s*(\d+)/);
    if (durationMatch) {
      duration = parseInt(durationMatch[1], 10);
    }
  }

  if (!duration) {
    duration = 600;
    log.debug(`    Duration unknown${isYoutube ? " (YouTube)" : ""}, using ${duration}s`);
  }

  log.debug(`    viewId=${viewId}, duration=${duration}s`);

  // Phase 1: Extract video sources
  const videoSources: string[] = [];
  const youtubeIds: string[] = [];

  // 1. Get src from <video> element
  const videoSrc = await page.evaluate(() => {
    const video = document.querySelector("video") as HTMLVideoElement | null;
    return video?.src || null;
  });
  if (videoSrc) videoSources.push(videoSrc);

  // 2. Get src from <source> elements
  const sourceSrcs = await page.evaluate(() => {
    const sources = Array.from(document.querySelectorAll("source"));
    return sources.map(s => s.src).filter((src): src is string => !!src);
  });
  videoSources.push(...sourceSrcs);

  // 3. Get src from <iframe> elements (YouTube, Vimeo, etc.)
  // Wait a bit for iframes to load
  await page.waitForTimeout(1000);
  const iframeSrcs = await page.evaluate(() => {
    const iframes = Array.from(document.querySelectorAll("iframe"));
    return iframes.map(f => f.src).filter((src): src is string => !!src && src.length > 0);
  });

  // Extract YouTube video IDs from iframe URLs
  for (const iframeSrc of iframeSrcs) {
    videoSources.push(iframeSrc);
    // Extract YouTube video ID
    const ytMatch = iframeSrc.match(/(?:youtube\.com\/(?:embed\/|v\/|watch\?v=)|youtu\.be\/)([a-zA-Z0-9_-]{11})/);
    if (ytMatch) {
      youtubeIds.push(ytMatch[1]);
    }
  }

  // 4. Check for blob/data URLs
  const hasBlobUrl = await page.evaluate(() => {
    const video = document.querySelector("video");
    const src = video?.src || "";
    return src.startsWith("blob:") || src.startsWith("data:");
  });

  // Deduplicate sources
  const uniqueSources = [...new Set(videoSources)];

  log.debug(`    Found ${uniqueSources.length} video source(s)`);
  if (uniqueSources.length > 0) {
    log.debug(`    Sources: ${uniqueSources.map(s => s.substring(0, 50) + (s.length > 50 ? "..." : "")).join(", ")}`);
  }
  if (youtubeIds.length > 0) {
    log.debug(`    YouTube IDs: ${youtubeIds.join(", ")}`);
  }
  if (hasBlobUrl) {
    log.warn(`    Video uses blob URL - cannot download directly`);
  }

  return {
    name,
    url: activityUrl,
    viewId,
    duration,
    existingPercent: 0,
    videoSources: uniqueSources,
    youtubeIds,
  };
}

// ── Video Download ─────────────────────────────────────────────────────────────

/**
 * Download a video from SuperVideo activity.
 * Supports direct video URLs (pluginfile.php) and YouTube videos.
 */
export async function downloadVideo(
  page: Page,
  metadata: { name: string; videoSources: string[]; youtubeIds?: string[] },
  outputPath: string,
  log: Logger
): Promise<{ success: boolean; path?: string; error?: string; type?: string }> {
  const { name, videoSources, youtubeIds } = metadata;

  log.info(`正在下載: ${name}`);

  // Priority 1: Direct video URL (pluginfile.php, .mp4, etc.)
  const directUrl = videoSources.find(s =>
    s.includes("pluginfile.php") ||
    s.endsWith(".mp4") ||
    s.endsWith(".webm") ||
    s.endsWith(".mov")
  );

  if (directUrl) {
    log.debug(`  類型: 直接下載 (${directUrl.substring(0, 60)}...)`);
    try {
      // Get session cookies from the page for authentication
      const cookies = await page.context().cookies();
      const cookieHeader = cookies
        .map(c => `${c.name}=${c.value}`)
        .join("; ");

      // Use native fetch with session cookies
      const response = await fetch(directUrl, {
        headers: {
          "Cookie": cookieHeader,
        },
      });

      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }

      // Get array buffer and convert to Uint8Array
      const arrayBuffer = await response.arrayBuffer();
      const uint8Array = new Uint8Array(arrayBuffer);

      // Write to file
      await Deno.writeFile(outputPath, uint8Array);

      return { success: true, path: outputPath, type: "direct" };
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      log.error(`  下載失敗: ${msg}`);
      return { success: false, error: msg };
    }
  }

  // Priority 2: YouTube video
  if (youtubeIds && youtubeIds.length > 0) {
    log.debug(`  類型: YouTube (ID: ${youtubeIds[0]})`);
    return {
      success: false,
      error: `YouTube 影片無法直接下載。請使用 yt-dlp: yt-dlp https://www.youtube.com/watch?v=${youtubeIds[0]}`,
      type: "youtube",
    };
  }

  // Priority 3: Other iframe/embedded video
  if (videoSources.length > 0) {
    log.debug(`  類型: 嵌入影片 (${videoSources[0].substring(0, 60)}...)`);
    return {
      success: false,
      error: "嵌入影片無法直接下載",
      type: "embedded",
    };
  }

  return {
    success: false,
    error: "未找到影片來源",
  };
}

// ── Progress Completion (from original progress.ts) ───────────────────────

/**
 * Build duration map array for video progress tracking.
 * Cached and scaled per duration to avoid repeated allocations.
 */
function buildDurationMap(duration: number): string {
  // Build the map array (0% to 100% in 1% increments)
  const map = Array.from({ length: 100 }, (_, i) => ({
    time: Math.round((duration * i) / 100),
    percent: i,
  }));
  return JSON.stringify(map);
}

/**
 * Complete a video using WS API (mobile service only).
 * Uses mod_supervideo_progress_save_mobile which is accessible via moodle_mobile_app service token.
 */
export async function completeVideoApi(
  session: { wsToken: string; moodleBaseUrl: string },
  video: { viewId: number; duration: number; url: string; cmid?: string }
): Promise<{ success: boolean; error?: string; result?: any }> {
  const { viewId, duration } = video;

  try {
    const result = await moodleApiCall<any>(
      session,
      "mod_supervideo_progress_save_mobile",  // Use mobile service specific function
      {
        view_id: viewId,
        currenttime: duration,
        duration: duration,
        percent: 100,
        mapa: buildDurationMap(duration),
      }
    );

    // Debug: log the full result
    // console.debug(`completeVideoApi result:`, JSON.stringify(result));

    const success = result?.[0]?.success === true || result?.success === true;
    return { success, error: success ? undefined : `API returned success=false, result=${JSON.stringify(result)}`, result };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    // console.debug(`completeVideoApi error: ${msg}`);
    return { success: false, error: msg };
  }
}

/**
 * Complete a video by forging progress AJAX call (legacy, requires browser).
 * Note: This uses sesskey-based AJAX which works for mod_supervideo_progress_save.
 */
export async function completeVideo(
  page: Page,
  session: SessionInfo,
  video: { viewId: number; duration: number; url: string; cmid?: string },
  log: Logger
): Promise<boolean> {
  const { viewId, duration } = video;

  const payload = {
    view_id: viewId,
    currenttime: duration,
    duration: duration,
    percent: 100,
    mapa: buildDurationMap(duration),
  };

  const url = `${session.moodleBaseUrl}/lib/ajax/service.php?sesskey=${session.sesskey}&info=mod_supervideo_progress_save`;
  const ajaxPayload = [{ index: 0, methodname: "mod_supervideo_progress_save", args: payload }];

  try {
    const result = await page.evaluate(
      async ({ url, ajaxPayload }) => {
        const res = await fetch(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(ajaxPayload),
        });
        return res.json();
      },
      { url, ajaxPayload }
    );

    if (result?.[0]?.error) {
      log.debug(`    Error: ${result[0].exception?.message ?? "Unknown error"}`);
      return false;
    }

    return true;
  } catch (err) {
    log.debug(`    Exception: ${err instanceof Error ? err.message : String(err)}`);
    return false;
  }
}

/**
 * Update activity completion status manually via WS API.
 * Used for marking resources as complete/incomplete.
 */
export async function updateActivityCompletionStatusManually(
  session: { wsToken: string; moodleBaseUrl: string },
  cmid: number,
  completed: boolean
): Promise<boolean> {
  try {
    const result = await moodleApiCall<any>(
      session,
      "core_completion_update_activity_completion_status_manually",
      {
        cmid: cmid,
        completed: completed ? 1 : 0,
      }
    );
    return result.status === true;
  } catch (e) {
    console.debug(`Failed to update completion status for cmid ${cmid}: ${e}`);
    return false;
  }
}

// ── Site Info (Get User ID) ───────────────────────────────────────────────────

/** Cache for site info to avoid redundant API calls */
let siteInfoCache: { userid: number; username: string; fullname: string; sitename: string } | null = null;
let siteInfoCacheTime = 0;
const SITE_INFO_CACHE_TTL = 5 * 60 * 1000; // 5 minutes

/**
 * Get site info including current user ID via pure WS API.
 * Results are cached for 5 minutes to avoid redundant calls.
 */
export async function getSiteInfoApi(
  session: { wsToken: string; moodleBaseUrl: string }
): Promise<{ userid: number; username: string; fullname: string; sitename: string }> {
  const now = Date.now();
  if (siteInfoCache && (now - siteInfoCacheTime) < SITE_INFO_CACHE_TTL) {
    return siteInfoCache;
  }

  const data = await moodleApiCall<any>(
    session,
    "core_webservice_get_site_info",
    {}
  );

  siteInfoCache = {
    userid: data.userid,
    username: data.username,
    fullname: data.fullname,
    sitename: data.sitename,
  };
  siteInfoCacheTime = now;

  return siteInfoCache;
}

/**
 * Get incomplete supervideos with completion tracking enabled via WS API.
 * Uses core_completion_get_activities_completion_status to get only videos that:
 * 1. Have completion tracking enabled (hascompletion: true)
 * 2. Are not yet completed (isoverallcomplete: false or state !== 1)
 */
export async function getIncompleteVideosApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseId: number
): Promise<Array<{ cmid: number; name: string; url: string }>> {
  // Get user ID
  const siteInfo = await getSiteInfoApi(session);

  // Get completion status for all activities
  const completionData = await moodleApiCall<any>(
    session,
    "core_completion_get_activities_completion_status",
    { courseid: courseId, userid: siteInfo.userid }
  );

  if (!completionData?.statuses) {
    return [];
  }

  // Get course contents to get URLs
  const contentsData = await moodleApiCall<unknown[]>(
    session,
    "core_course_get_contents",
    { courseid: courseId }
  );

  // Create a map of cmid to URL
  const urlMap = new Map<number, string>();
  for (const section of (contentsData as any[]) || []) {
    if (!section.modules) continue;
    for (const module of section.modules) {
      if (module.id) {
        urlMap.set(module.id, module.url);
      }
    }
  }

  // Filter for incomplete supervideos with completion tracking enabled
  const incompleteVideos: Array<{ cmid: number; name: string; url: string }> = [];
  for (const status of completionData.statuses) {
    // Only supervideo modules
    if (status.modname !== "supervideo") continue;

    // Must have completion enabled
    if (!status.hascompletion) continue;

    // Must be incomplete
    if (status.isoverallcomplete === true || status.state === 1) continue;

    const url = urlMap.get(status.cmid) || "";
    incompleteVideos.push({
      cmid: status.cmid,
      name: status.name,
      url,
    });
  }

  return incompleteVideos;
}

// ── Videos via WS API ─────────────────────────────────────────────────────────

/**
 * Get course contents and filter for SuperVideo modules via pure WS API.
 */
export async function getSupervideosInCourseApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseId: number
): Promise<SuperVideoModule[]> {
  const data = await moodleApiCall<unknown[]>(
    session,
    "core_course_get_contents",
    { courseid: courseId }
  );

  const videos: SuperVideoModule[] = [];

  // data is an array of sections
  for (const section of (data as any[]) || []) {
    // Each section has modules array
    if (!section.modules) continue;

    for (const module of section.modules) {
      // Filter for SuperVideo modname
      if (module.modname === "supervideo") {
        videos.push({
          cmid: module.id.toString(),
          name: module.name,
          url: module.url,
          instance: module.instance,  // supervideo instance id (not cmid!)
          isComplete: false, // Will be updated from completion API
        });
      }
    }
  }

  // Get completion status using core_completion_get_activities_completion_status
  try {
    // First get user ID
    const siteInfo = await getSiteInfoApi(session);

    const completionData = await moodleApiCall<any>(
      session,
      "core_completion_get_activities_completion_status",
      { courseid: courseId, userid: siteInfo.userid }
    );

    // Create a map of cmid to completion status
    const completionMap = new Map<number, boolean>();
    if (completionData?.statuses) {
      for (const status of completionData.statuses) {
        // Only include modules that have completion enabled
        if (status.hascompletion) {
          completionMap.set(status.cmid, status.isoverallcomplete === true);
        }
      }
    }

    // Update isComplete based on completion data
    for (const video of videos) {
      const cmid = parseInt(video.cmid, 10);
      if (completionMap.has(cmid)) {
        video.isComplete = completionMap.get(cmid) ?? false;
      }
    }
  } catch (e) {
    // If completion API fails, continue with isComplete=false
    console.debug(`Failed to get completion status: ${e}`);
  }

  return videos;
}

// ── Quizzes via WS API ────────────────────────────────────────────────────────

/**
 * Get user attempts for given quiz IDs via WS API.
 * Returns a map of quiz ID -> { finished: boolean, attemptsUsed: number }.
 * Note: mod_quiz_get_user_attempts only accepts a single quizid, so we query in parallel.
 */
async function getUserQuizAttemptInfo(
  session: { wsToken: string; moodleBaseUrl: string },
  quizIds: number[]
): Promise<Map<number, { finished: boolean; attemptsUsed: number }>> {
  if (quizIds.length === 0) return new Map();

  const info = new Map<number, { finished: boolean; attemptsUsed: number }>();

  // Query each quiz in parallel (API only accepts single quizid)
  const results = await Promise.allSettled(
    quizIds.map(quizId =>
      moodleApiCall<{ attempts?: unknown[] }>(
        session,
        "mod_quiz_get_user_attempts",
        { quizid: quizId }
      )
    )
  );

  for (let i = 0; i < quizIds.length; i++) {
    const quizId = quizIds[i];
    const result = results[i];
    if (result.status === "fulfilled" && result.value?.attempts) {
      let used = 0;
      let hasFinished = false;
      for (const a of result.value.attempts as any[]) {
        used++;
        if (a.state === "finished") hasFinished = true;
      }
      info.set(quizId, { finished: hasFinished, attemptsUsed: used });
    } else {
      info.set(quizId, { finished: false, attemptsUsed: 0 });
    }
  }

  return info;
}

/**
 * Extended QuizModule with courseId for API responses.
 */
export interface QuizModuleWithCourse extends QuizModule {
  courseId: number;
}

/**
 * Get quizzes in courses via pure WS API.
 */
export async function getQuizzesByCoursesApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseIds: number[]
): Promise<QuizModuleWithCourse[]> {
  if (courseIds.length === 0) return [];

  const data = await moodleApiCall<{ quizzes?: unknown[] }>(
    session,
    "mod_quiz_get_quizzes_by_courses",
    { courseids: courseIds }
  );


  const quizzes = (data?.quizzes ?? []) as any[];

  // Fetch user attempts to determine completion status and left attempts
  const quizIds = quizzes.map(q => q.id);
  const attemptInfo = await getUserQuizAttemptInfo(session, quizIds);

  return quizzes.map(q => {
    const info = attemptInfo.get(q.id);
    return {
      quizid: q.id.toString(),
      name: q.name,
      url: q.viewurl,
      intro: q.intro,
      isComplete: info?.finished ?? false,
      attemptsUsed: info?.attemptsUsed ?? 0,
      timeClose: q.timeclose,
      maxAttempts: q.attempts,
      courseId: q.course,
    };
  });
}

/**
 * Start a new quiz attempt via pure WS API.
 */
export async function startQuizAttemptApi(
  session: { wsToken: string; moodleBaseUrl: string },
  quizId: string,
  options: { forcenew?: boolean; precheck?: boolean } = {}
): Promise<QuizStartResult> {
  const params: Record<string, unknown> = {
    quizid: parseInt(quizId, 10),
    forcenew: options.forcenew ? 1 : 0,
  };

  if (options.precheck) {
    params.precheck = 1;
  }

  const data = await moodleApiCall<{
    attempt?: unknown;
    page?: number;
    messages?: string[];
  }>(session, "mod_quiz_start_attempt", params);

  if (!data?.attempt) {
    throw new Error("No attempt data returned");
  }

  const attempt = data.attempt as any;
  const attemptId = attempt.id || attempt.attempt;
  return {
    attempt: {
      attempt: attemptId,
      attemptid: attemptId,
      quizid: attempt.quizid,
      userid: attempt.userid,
      attemptnumber: attempt.attemptnumber,
      state: attempt.state,
      timestart: attempt.timestart,
      timefinish: attempt.timefinish,
      preview: attempt.preview === 1,
    },
    page: data.page,
    messages: data.messages,
  };
}

/**
 * Get quiz attempt data including questions via pure WS API.
 */
export async function getAllQuizAttemptDataApi(
  session: { wsToken: string; moodleBaseUrl: string },
  attemptId: number
): Promise<QuizAttemptData> {
  const firstPage = await getQuizAttemptDataApi(session, attemptId, 0);

  // Moodle re-indexes question keys per page (always starts at 0),
  // so we must re-key by actual slot number to avoid overwrites.
  const allQuestions: Record<number, QuizQuestion> = {};
  for (const q of Object.values(firstPage.questions)) {
    allQuestions[q.slot] = q;
  }

  let nextPage = firstPage.nextpage;
  while (nextPage !== undefined && nextPage !== null && nextPage !== -1) {
    const pageData = await getQuizAttemptDataApi(session, attemptId, nextPage);
    for (const q of Object.values(pageData.questions)) {
      allQuestions[q.slot] = q;
    }
    nextPage = pageData.nextpage;
  }

  return { ...firstPage, questions: allQuestions, nextpage: undefined };
}

export async function getQuizAttemptDataApi(
  session: { wsToken: string; moodleBaseUrl: string },
  attemptId: number,
  page: number = 0
): Promise<QuizAttemptData> {
  const data = await moodleApiCall<{
    attempt?: unknown;
    questions?: Record<string, unknown>;
    nextpage?: number;
    prevpage?: number;
  }>(session, "mod_quiz_get_attempt_data", { attemptid: attemptId, page });

  if (!data?.attempt || !data?.questions) {
    throw new Error("Invalid attempt data response");
  }

  const attempt = data.attempt as any;
  const attemptIdValue = attempt.id || attempt.attempt;
  const questions: Record<number, QuizQuestion> = {};

  for (const [slot, question] of Object.entries(data.questions)) {
    questions[parseInt(slot, 10)] = {
      slot: (question as any).slot,
      type: (question as any).type,
      id: (question as any).id,
      maxmark: (question as any).maxmark,
      page: (question as any).page,
      quizid: (question as any).quizid,
      html: (question as any).html,
      status: (question as any).status,
      stateclass: (question as any).stateclass,
      sequencecheck: (question as any).sequencecheck,
      questionnumber: (question as any).questionnumber,
    } as QuizQuestion;
  }

  return {
    attempt: {
      attempt: attemptIdValue,
      attemptid: attemptIdValue,
      uniqueid: attempt.uniqueid,
      quizid: attempt.quizid,
      userid: attempt.userid,
      attemptnumber: attempt.attemptnumber,
      state: attempt.state,
      timestart: attempt.timestart,
      timefinish: attempt.timefinish,
    },
    questions,
    nextpage: data.nextpage,
    prevpage: data.prevpage,
  };
}

/**
 * Process (save answers / finish) a quiz attempt via WS API.
 *
 * Answer formats:
 * - Single choice (multichoice): { slot, answer: "0" }
 * - Multiple choice (multichoices): { slot, answer: "0,2,3" } (comma-separated choice indices)
 * - Short answer (shortanswer): { slot, answer: "text answer" }
 *
 * @param session - WS session
 * @param attemptId - The attempt ID
 * @param uniqueId - The usage attempt uniqueid (from attempt data)
 * @param answers - Array of { slot, answer } pairs
 * @param sequenceChecks - Map of slot -> sequencecheck value (required for deferredfeedback)
 * @param finish - Whether to finish the attempt after saving
 */
export async function processQuizAttemptApi(
  session: { wsToken: string; moodleBaseUrl: string },
  attemptId: number,
  uniqueId: number,
  answers: Array<{ slot: number; answer: string }>,
  sequenceChecks: Map<number, number>,
  finish: boolean = true
): Promise<{ state: string; warnings?: unknown[] }> {
  const params: Record<string, unknown> = {
    attemptid: attemptId,
    finishattempt: finish ? 1 : 0,
  };

  let i = 0;
  for (const a of answers) {
    // Include sequencecheck first (required for deferredfeedback quizzes)
    const seq = sequenceChecks.get(a.slot);
    if (seq !== undefined) {
      params[`data[${i}][name]`] = `q${uniqueId}:${a.slot}_:sequencecheck`;
      params[`data[${i}][value]`] = seq.toString();
      i++;
    }

    // Detect answer format:
    // Comma-separated numeric values = multichoices (checkboxes)
    // Single numeric value = multichoice (radio)
    // Non-numeric text = shortanswer
    if (/^\d+(,\d+)*$/.test(a.answer) && a.answer.includes(",")) {
      // Multichoices: send each choice as qXXX:Y_choiceN with value 1
      const choices = a.answer.split(",");
      for (const choice of choices) {
        params[`data[${i}][name]`] = `q${uniqueId}:${a.slot}_choice${choice}`;
        params[`data[${i}][value]`] = "1";
        i++;
      }
    } else {
      // Single choice or shortanswer: send as qXXX:Y_answer
      params[`data[${i}][name]`] = `q${uniqueId}:${a.slot}_answer`;
      params[`data[${i}][value]`] = a.answer;
      i++;
    }
  }

  const data = await moodleApiCall<{ state?: string; warnings?: unknown[] }>(
    session,
    "mod_quiz_process_attempt",
    params
  );

  return { state: data?.state ?? "unknown", warnings: data?.warnings };
}

// ── Materials via WS API ──────────────────────────────────────────────────────

/**
 * Get resources in courses via pure WS API.
 */
export async function getResourcesByCoursesApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseIds: number[]
): Promise<ResourceModule[]> {
  if (courseIds.length === 0) return [];

  const data = await moodleApiCall<{ resources?: unknown[] }>(
    session,
    "mod_resource_get_resources_by_courses",
    { courseids: courseIds }
  );

  return (data?.resources ?? []).map((r: any) => {
    // Extract file info from contentfiles array
    const firstFile = r.contentfiles?.[0];
    return {
      cmid: r.coursemodule?.toString() ?? r.id?.toString() ?? "",
      name: r.name,
      url: firstFile?.fileurl ?? "",
      courseId: r.course,
      modType: "resource", // This API only returns resources
      mimetype: firstFile?.mimetype,
      filesize: firstFile?.filesize,
      modified: r.timemodified,
    };
  });
}

// ── Assignments via WS API ─────────────────────────────────────────────────────

/**
 * Extended AssignmentModule with courseId for API responses.
 */
export interface AssignmentModuleWithCourse {
  id: number; // Assignment instance ID (for mod_assign_get_submission_status)
  cmid: string; // Course module ID
  name: string;
  url: string;
  courseId: number;
  duedate?: number;
  cutoffdate?: number;
  allowSubmissionsFromDate?: number;
  gradingduedate?: number;
  lateSubmission?: boolean;
  extensionduedate?: number;
}

/**
 * Get assignments in courses via pure WS API.
 */
export async function getAssignmentsByCoursesApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseIds: number[]
): Promise<AssignmentModuleWithCourse[]> {
  if (courseIds.length === 0) return [];

  const data = await moodleApiCall<{ courses?: unknown[] }>(
    session,
    "mod_assign_get_assignments",
    { courseids: courseIds }
  );

  const assignments: AssignmentModuleWithCourse[] = [];

  // The API returns an array of courses, each containing assignments
  for (const course of (data?.courses ?? []) as any[]) {
    if (!course.assignments) continue;

    for (const a of course.assignments) {
      assignments.push({
        id: a.id,
        cmid: a.cmid?.toString() ?? "",
        name: a.name,
        url: a.viewurl ?? "",
        courseId: course.id,
        duedate: a.duedate,
        cutoffdate: a.cutoffdate,
        allowSubmissionsFromDate: a.allowsubmissionsfromdate,
        gradingduedate: a.gradingduedate,
        lateSubmission: a.latesubmissions ? true : false,
        extensionduedate: a.extensionduedate,
      });
    }
  }

  return assignments;
}

/**
 * Get assignment submission status via pure WS API.
 */
export async function getSubmissionStatusApi(
  session: { wsToken: string; moodleBaseUrl: string },
  assignmentId: number
): Promise<{
  submitted: boolean;
  graded: boolean;
  grader: string | null;
  grade: string | null;
  feedback: string | null;
  lastModified: number | null;
  extensions: Array<{ id: number; filename: string; filesize: number }>;
}> {
  const siteInfo = await getSiteInfoApi(session);

  const data = await moodleApiCall<any>(
    session,
    "mod_assign_get_submission_status",
    {
      assignid: assignmentId,
      userid: siteInfo.userid,
    }
  );

  const lastAttempt = data?.lastattempt;
  // Note: API returns "submission" (singular), not "submissions" (plural)
  const submission = lastAttempt?.submission;

  // Find file submissions from submission plugins
  const plugins = submission?.plugins || [];
  const filePlugin = plugins.find((p: any) => p.type === "file");
  const extensions = (filePlugin?.fileareas || [])
    .flatMap((fa: any) => fa?.files || [])
    .map((f: any) => ({
      id: f.id,
      filename: f.filename,
      filesize: f.filesize,
    }));

  // Get feedback from the separate feedback object
  const feedback = data?.feedback;
  const commentsPlugin = feedback?.plugins?.find((p: any) => p.type === "comments");
  const commentText = commentsPlugin?.editorfields?.find((e: any) => e.name === "comments")?.text || null;

  return {
    submitted: submission?.status === "submitted",
    graded: lastAttempt?.gradingstatus === "graded",
    grader: feedback?.gradername || null,
    grade: feedback?.gradefordisplay || null,
    feedback: commentText,
    lastModified: submission?.timemodified || null,
    extensions,
  };
}

/**
 * Save/submit an assignment via pure WS API.
 * Supports online text submissions and file submissions (by draft ID).
 */
export async function saveSubmissionApi(
  session: { wsToken: string; moodleBaseUrl: string },
  assignmentId: number,
  options: {
    onlineText?: { text: string; format?: number };
    fileId?: number; // Draft file ID from file upload
    plugintype?: "onlinetext" | "file" | "comments";
  }
): Promise<{ success: boolean; error?: string }> {
  try {
    const siteInfo = await getSiteInfoApi(session);

    // Build plugins array based on submission type
    const plugins: any[] = [];

    if (options.onlineText) {
      plugins.push({
        type: "onlinetext",
        online_text: {
          text: options.onlineText.text,
          format: options.onlineText.format ?? 1,
          itemid: 0,
        },
      });
    }

    if (options.fileId !== undefined) {
      plugins.push({
        type: "file",
        files_filemanager: options.fileId,
      });
    }

    await moodleApiCall<any>(
      session,
      "mod_assign_save_submission",
      {
        assignmentid: assignmentId,
        userid: siteInfo.userid,
        plugins,
      }
    );

    return { success: true };
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : String(e) };
  }
}

// ── File Upload via WS API ──────────────────────────────────────────────────────

/**
 * Generate a unique draft item ID.
 * Uses timestamp (last 8 digits) to ensure uniqueness.
 */
export function generateDraftItemId(): number {
  // Use current timestamp in seconds, take last 8 digits
  return Math.floor(Date.now() / 1000) % 100000000;
}

/**
 * Upload a file to Moodle draft area via pure WS API.
 * This is the first step before submitting files to assignments, forums, etc.
 *
 * Note: We generate our own draft item ID instead of asking Moodle for one.
 */
export async function uploadFileApi(
  session: { wsToken: string; moodleBaseUrl: string },
  filePath: string,
  options?: {
    draftId?: number; // Use specific draft ID, or auto-generate
    filename?: string;
    filepath?: string; // Draft area path (default: "/")
  }
): Promise<{ success: boolean; draftId?: number; error?: string }> {
  try {
    // Generate or use provided draft ID
    const draftItemId = options?.draftId ?? generateDraftItemId();

    // Read file content using fs.promises
    const fileContent = await fs.promises.readFile(filePath);
    const fileName = options?.filename || filePath.split(/[/\\]/).pop() || "unknown";

    // Get site info for user context
    const siteInfo = await getSiteInfoApi(session);
    const userContextId = getUserContextId(siteInfo.userid);

    // Prepare multipart form data
    const formData = new FormData();
    formData.append("token", session.wsToken);
    formData.append("file", new Blob([new Uint8Array(fileContent)]), fileName);
    formData.append("filepath", options?.filepath || "/");
    formData.append("itemid", String(draftItemId)); // Use our generated draft ID
    formData.append("contextid", String(userContextId)); // Use calculated user context
    formData.append("component", "user");
    formData.append("filearea", "draft");
    formData.append("qformat", ""); // Not used

    // Upload via upload.php (uses multipart/form-data)
    const url = `${session.moodleBaseUrl}/webservice/upload.php`;

    const response = await fetch(url, {
      method: "POST",
      body: formData,
    });

    if (!response.ok) {
      return { success: false, error: `HTTP ${response.status}` };
    }

    const result = await response.json();

    console.debug(`[DEBUG] upload.php response:`, JSON.stringify(result, null, 2));

    // Check for errors in response
    if (result?.error) {
      return { success: false, error: result.message ?? result.error ?? "Upload failed" };
    }

    // Success - return the draft ID we used
    return { success: true, draftId: draftItemId };
  } catch (e) {
    return { success: false, error: e instanceof Error ? e.message : String(e) };
  }
}

/**
 * Calculate user context ID in Moodle.
 * User context ID = (userid * CONTEXT_DEPTH) + CONTEXT_USER
 * Where CONTEXT_DEPTH = 10 and CONTEXT_USER = 30
 */
function getUserContextId(userId: number): number {
  return userId * 10 + 30;
}

/**
 * Get user's private files (not draft) via pure WS API.
 * Draft files are temporary and cannot be listed, but private files can.
 */
export async function getDraftFilesApi(
  session: { wsToken: string; moodleBaseUrl: string }
): Promise<Array<{
  itemId: number;
  filename: string;
  filesize: number;
  filepath: string;
  timeModified: number;
  url: string;
}>> {
  const siteInfo = await getSiteInfoApi(session);
  const userContextId = getUserContextId(siteInfo.userid);

  const data = await moodleApiCall<any>(
    session,
    "core_files_get_files",
    {
      contextid: userContextId,
      component: "user",
      filearea: "private",
      itemid: 0,
      filepath: "/",
      modified: null,
    }
  );

  console.debug(`[DEBUG] core_files_get_files response:`, JSON.stringify(data, null, 2));

  // The API returns a parents array with files inside
  const files: any[] = data?.parents?.[0]?.files || data?.files || [];

  return files.map((f: any) => ({
    itemId: f.itemid || 0,
    filename: f.filename || "",
    filesize: f.filesize || 0,
    filepath: f.filepath || "/",
    timeModified: f.timemodified || 0,
    url: f.url || "",
  }));
}

// ── Messages via WS API ───────────────────────────────────────────────────────

export interface Message {
  id: number;
  useridfrom: number;
  useridto: number;
  subject: string;
  text: string;
  timecreated: number;
  fullmessage: string;
  fullmessageformat: number;
  fullmessagehtml: string;
}

/**
 * Get messages for the current user via pure WS API.
 */
export async function getMessagesApi(
  session: { wsToken: string; moodleBaseUrl: string },
  userIdTo: number,
  options: { useridfrom?: number; read?: boolean; limitfrom?: number; limitnum?: number } = {}
): Promise<Message[]> {
  const data = await moodleApiCall<{ messages?: unknown[] }>(
    session,
    "core_message_get_messages",
    { useridto: userIdTo, ...options }
  );

  return (data?.messages ?? []).map((m: any) => ({
    id: m.id,
    useridfrom: m.useridfrom,
    useridto: m.useridto,
    subject: m.subject,
    text: m.smallmessage,
    timecreated: m.timecreated,
    fullmessage: m.fullmessage,
    fullmessageformat: m.fullmessageformat,
    fullmessagehtml: m.fullmessagehtml,
  }));
}
