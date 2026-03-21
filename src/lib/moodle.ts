import type { Page } from "playwright-core";
import { parse } from "node-html-parser";
import type {
  SessionInfo,
  Logger,
  EnrolledCourse,
  SuperVideoModule,
  QuizModule,
  ForumModule,
  ResourceModule,
  ForumDiscussion,
  ForumPost,
  CalendarEvent,
  CourseGrade,
} from "./types.ts";

// ── HTML Parsing Helpers ──────────────────────────────────────────────────

/**
 * Get the HTML content of a page and parse it.
 */
async function fetchAndParse(page: Page, url: string): Promise<ReturnType<typeof parse>> {
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30000 });
  const content = await page.content();
  return parse(content);
}

// ── Core Moodle AJAX Wrapper ───────────────────────────────────────────

/**
 * Moodle WS API functions that are known to work via /webservice/rest/server.php
 * Other functions should use the sesskey-based AJAX API.
 */
const WS_API_FUNCTIONS = new Set([
  "mod_forum_get_forums_by_courses",
  "mod_forum_get_forum_discussions",
  "mod_forum_get_forum_discussion_posts",
  "gradereport_user_get_grade_items",
  "core_calendar_get_calendar_events",
  "core_course_get_contents",
  "mod_quiz_get_quizzes_by_courses",
  "mod_resource_get_resources_by_courses",
  "core_message_get_messages",
  "core_webservice_get_site_info",
]);

/**
 * Convert args to URLSearchParams, handling arrays properly for Moodle WS API.
 * Moodle expects array parameters as: courseids[0]=1&courseids[1]=2
 */
function buildWsParams(args: Record<string, unknown>): URLSearchParams {
  const params = new URLSearchParams();

  for (const [key, value] of Object.entries(args)) {
    if (Array.isArray(value)) {
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
    throw new Error(`WS Token required for API call: ${methodname}`);
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
    fullname: c.fullname,
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
    fullname: c.fullname,
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

  log.debug(`  Course state returned ${cms.length} modules`);

  // Debug: log first few modules
  for (let i = 0; i < Math.min(3, cms.length); i++) {
    log.debug(`  Module ${i}: ${JSON.stringify(cms[i])}`);
  }

  const allSupervideos = cms.filter((cm: any) => cm.module === "supervideo" || cm.modname === "supervideo");

  log.debug(`  Found ${allSupervideos.length} supervideo modules`);

  const incomplete = allSupervideos.filter(
    (cm: any) => !("isoverallcomplete" in cm && cm.isoverallcomplete)
  );

  log.debug(
    `  SuperVideo: ${allSupervideos.length} total, ${incomplete.length} incomplete`
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

// ── Quiz Operations ───────────────────────────────────────────────────────

/**
 * Get all Quiz modules in a course.
 */
export async function getQuizzesInCourse(
  page: Page,
  session: SessionInfo,
  courseId: number,
  log: Logger
): Promise<QuizModule[]> {
  const state = await getCourseState(page, session, courseId);
  const cms: any[] = state?.cm ?? [];

  const allQuizzes = cms.filter((cm: any) => cm.module === "quiz");
  const available = allQuizzes.filter(
    (cm: any) => !("isoverallcomplete" in cm) || !cm.isoverallcomplete
  );

  log.debug(
    `  Quiz: ${allQuizzes.length} total, ${available.length} available`
  );

  return available.map((cm: any) => ({
    cmid: cm.cmid?.toString() ?? cm.id?.toString() ?? "",
    name: cm.name,
    url: cm.url,
    isComplete: !!cm.isoverallcomplete,
    timeOpen: cm.timeopen,
    timeClose: cm.timeclose,
  }));
}

// ── Forum Operations ──────────────────────────────────────────────────────

/**
 * Get all forum modules in a course.
 * If WS token is available, fetches forum IDs directly via WS API.
 */
export async function getForumsInCourse(
  page: Page,
  session: SessionInfo,
  courseId: number,
  log: Logger
): Promise<ForumModule[]> {
  // First get basic forum info from course state
  const state = await getCourseState(page, session, courseId);
  const cms: any[] = state?.cm ?? [];
  const forums = cms.filter((cm: any) => cm.module === "forum");

  log.debug(`  Found ${forums.length} forum${forums.length === 1 ? "" : "s"}.`);

  const result: ForumModule[] = forums.map((cm: any) => ({
    cmid: cm.cmid?.toString() ?? cm.id?.toString() ?? "",
    forumId: 0,
    name: cm.name,
    url: cm.url,
    courseId,
    forumType: cm.modname,
  }));

  // If WS token is available, fetch forum IDs directly
  if (session.wsToken && forums.length > 0) {
    try {
      const wsForums = await moodleAjax<any[]>(
        page,
        session,
        "mod_forum_get_forums_by_courses",
        { courseids: [courseId] }
      );

      // Create maps for lookup by different fields
      const byId = new Map<number, number>(); // cmid -> forum id
      const byName = new Map<string, number>(); // name -> forum id

      for (const wsForum of wsForums || []) {
        if (wsForum.cmid) {
          byId.set(wsForum.cmid, wsForum.id);
        }
        if (wsForum.name) {
          byName.set(wsForum.name, wsForum.id);
        }
      }

      // Merge forum IDs into result
      for (const forum of result) {
        const cmid = parseInt(forum.cmid, 10);
        if (byId.has(cmid)) {
          forum.forumId = byId.get(cmid)!;
        } else if (byName.has(forum.name)) {
          forum.forumId = byName.get(forum.name)!;
        }
      }

      const matchedCount = result.filter(f => f.forumId > 0).length;
      log.debug(`  WS API provided forum IDs for ${matchedCount}/${result.length} forums.`);
    } catch (e) {
      log.debug(`  WS API forum lookup failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  return result;
}

/**
 * Get all forums via pure WS API (no browser required).
 * Fast and lightweight - uses HTTP fetch directly.
 */
export async function getForumsApi(
  session: { wsToken: string; moodleBaseUrl: string },
  courseIds: number[]
): Promise<Array<{ id: number; cmid: number; name: string; courseid: number }>> {
  const data = await moodleApiCall<any[]>(
    session,
    "mod_forum_get_forums_by_courses",
    { courseids: courseIds }
  );

  return (data ?? []).map((f: any) => ({
    id: f.id,
    cmid: f.cmid,
    name: f.name,
    courseid: f.course,  // API returns 'course' not 'courseid'
  }));
}

/**
 * Extract forum ID from forum page.
 * First tries to find it in embedded page data, then falls back to
 * extracting it from discussion posts API.
 */
export async function getForumIdFromPage(
  page: Page,
  cmid: number,
  session?: SessionInfo
): Promise<number | null> {
  try {
    await page.goto(
      `https://ilearning.cycu.edu.tw/mod/forum/view.php?id=${cmid}`,
      { waitUntil: "domcontentloaded", timeout: 30000 }
    );

    // First try: extract from page HTML
    const forumId = await page.evaluate(() => {
      // Try multiple patterns to find the forum ID
      const patterns = [
        /"forumid":(\d+)/,
        /"forumId":(\d+)/,
        /forumid=(\d+)/,
        /data-forum-id="(\d+)"/,
      ];

      const html = document.body.innerHTML;
      for (const pattern of patterns) {
        const match = html.match(pattern);
        if (match) return parseInt(match[1], 10);
      }

      // Try to find it in a script tag with forum configuration
      const scripts = Array.from(document.querySelectorAll('script'));
      for (const script of scripts) {
        const text = script.textContent || '';
        const match = text.match(/"forumid":(\d+)/);
        if (match) return parseInt(match[1], 10);
      }

      // Try to find from discussion links - extract from API data embedded in page
      const discussLinks = Array.from(document.querySelectorAll('a[href*="discuss.php"]'));
      for (const link of discussLinks) {
        const href = (link as HTMLAnchorElement).href;
        // The discussion page might have forum info
        const dMatch = href.match(/d=(\d+)/);
        if (dMatch) {
          // Try to find parent element with forum data
          let parent = link.parentElement;
          let depth = 0;
          while (parent && depth < 10) {
            const parentHtml = parent.innerHTML;
            const fMatch = parentHtml.match(/"forum":(\d+)/);
            if (fMatch) return parseInt(fMatch[1], 10);
            parent = parent.parentElement;
            depth++;
          }
        }
      }

      return null;
    });

    if (forumId) return forumId;

    // Fallback: if session is provided, try to get instance ID from discussion posts
    if (session) {
      // Get first discussion ID from page
      const firstDiscussionId = await page.evaluate(() => {
        const link = document.querySelector('a[href*="discuss.php"]');
        if (!link) return null;
        const href = (link as HTMLAnchorElement).href;
        const match = href.match(/d=(\d+)/);
        return match ? parseInt(match[1], 10) : null;
      });

      if (firstDiscussionId) {
        // Try to get posts and extract forum ID from response
        const data = await moodleAjax<{ posts?: unknown[] }>(
          page,
          session,
          "mod_forum_get_forum_discussion_posts",
          {
            discussionid: firstDiscussionId,
          }
        );

        if (data?.posts && data.posts.length > 0) {
          const firstPost = data.posts[0] as any;
          if (firstPost.forum) {
            return firstPost.forum;
          }
        }
      }
    }

    return null;
  } catch {
    return null;
  }
}

/**
 * Get forums by course IDs via AJAX.
 * Returns forum instance IDs directly from Moodle API.
 * This is the cleanest way to get forum instance IDs.
 */
export async function getForumsByCourseIds(
  page: Page,
  session: SessionInfo,
  courseIds: number[]
): Promise<Array<{ id: number; course: number; name: string }>> {
  if (courseIds.length === 0) return [];

  try {
    const data = await moodleAjax<Array<{ id: number; course: number; name: string }>>(
      page,
      session,
      "mod_forum_get_forums_by_courses",
      {
        courseids: courseIds,
      }
    );

    return data ?? [];
  } catch (e: any) {
    // Re-throw with more context
    throw new Error(`mod_forum_get_forums_by_courses failed: ${e?.message || e}`);
  }
}

/**
 * Get discussions in a forum via AJAX.
 * Note: Requires forum instance ID, not cmid. Use getForumsByCourseIds() first.
 */
export async function getForumDiscussions(
  page: Page,
  session: SessionInfo,
  forumId: number
): Promise<ForumDiscussion[]> {
  const data = await moodleAjax<{ discussions?: unknown[] }>(
    page,
    session,
    "mod_forum_get_forum_discussions",
    {
      forumid: forumId,
    }
  );

  return (data?.discussions ?? []).map((d: any) => ({
    id: d.id,
    forumId: d.forum,
    name: d.name,
    firstPostId: d.firstpost,
    userId: d.userid,
    groupId: d.groupid,
    timedue: d.timedue,
    timeModified: d.timemodified,
    userModified: d.usermodified,
    postCount: d.numdiscussion,
    unread: d.unread,
  }));
}

/**
 * Get posts in a discussion via AJAX.
 */
export async function getDiscussionPosts(
  page: Page,
  session: SessionInfo,
  discussionId: number
): Promise<ForumPost[]> {
  try {
    const data = await moodleAjax<{ posts?: unknown[] }>(
      page,
      session,
      "mod_forum_get_forum_discussion_posts",
      {
        discussionid: discussionId,
      }
    );

    if (!data?.posts || data.posts.length === 0) {
      return [];
    }

    return (data.posts as any[]).map((p: any) => ({
      id: p.id,
      subject: p.subject || "",
      author: p.author?.fullname ?? p.username ?? "Unknown",
      authorId: p.userid,
      created: p.created,
      modified: p.modified,
      message: p.message || "",
      discussionId: p.discussion,
      unread: p.unread ?? false,
    }));
  } catch (error) {
    // Return empty array on error instead of throwing
    // This allows commands to gracefully handle inaccessible discussions
    return [];
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

// ── Grade Operations ──────────────────────────────────────────────────────

/**
 * Get course grades for the current user via AJAX.
 */
export async function getCourseGrades(
  page: Page,
  session: SessionInfo,
  courseId: number
): Promise<CourseGrade> {
  const data = await moodleAjax<{ usergrades?: unknown[] }>(
    page,
    session,
    "gradereport_user_get_grade_items",
    {
      courseid: courseId,
    }
  );

  const userGrades = data?.usergrades?.[0] as any;
  if (!userGrades) {
    return { courseId, courseName: "", items: [] };
  }

  return {
    courseId,
    courseName: userGrades.coursefullname ?? "",
    grade: userGrades.grade,
    gradeFormatted: userGrades.gradeformatted,
    rank: userGrades.rank,
    totalUsers: userGrades.totalusers,
    items: (userGrades.gradeitems ?? []).map((item: any) => ({
      id: item.id,
      name: item.itemname || item.itemmodule,
      grade: item.grade,
      gradeFormatted: item.gradeformatted,
      range: item.graderangeformatted,
      percentage: item.percentage,
      weight: item.weight,
      feedback: item.feedback,
      graded: !!item.graded,
    })),
  };
}

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
    {
      courseid: courseId,
    }
  );

  const userGrades = data?.usergrades?.[0] as any;
  if (!userGrades) {
    return { courseId, courseName: "", items: [] };
  }

  return {
    courseId,
    courseName: userGrades.coursefullname ?? "",
    grade: userGrades.grade,
    gradeFormatted: userGrades.gradeformatted,
    rank: userGrades.rank,
    totalUsers: userGrades.totalusers,
    items: (userGrades.gradeitems ?? []).map((item: any) => ({
      id: item.id,
      name: item.itemname || item.itemmodule,
      grade: item.grade,
      gradeFormatted: item.gradeformatted,
      range: item.graderangeformatted,
      percentage: item.percentage,
      weight: item.weight,
      feedback: item.feedback,
      graded: !!item.graded,
    })),
  };
}

// ── Calendar Operations ───────────────────────────────────────────────────

/**
 * Get calendar events via AJAX.
 */
export async function getCalendarEvents(
  page: Page,
  session: SessionInfo,
  options: {
    courseId?: number;
    startTime?: number;
    endTime?: number;
    events?: { courseid?: number; groupid?: number; categoryid?: number }[];
  } = {}
): Promise<CalendarEvent[]> {
  const data = await moodleAjax<{ events?: unknown[] }>(
    page,
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

// ── Video Metadata (from original course.ts) ───────────────────────────────

/**
 * Visit a SuperVideo activity page and extract view_id + duration.
 */
export async function getVideoMetadata(
  page: Page,
  activityUrl: string,
  log: Logger
): Promise<{ name: string; url: string; viewId: number; duration: number; existingPercent: number; videoSources: string[]; youtubeIds?: string[] }> {
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

      // Write to file using Deno
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
 * Complete a video by forging progress AJAX call.
 */
export async function completeVideo(
  page: Page,
  session: SessionInfo,
  video: { viewId: number; duration: number; url: string; cmid?: string },
  log: Logger
): Promise<boolean> {
  const { viewId, duration } = video;

  // Build duration map array (required by Moodle)
  const map = Array.from({ length: 100 }, (_, i) => ({
    time: Math.round((duration * i) / 100),
    percent: i,
  }));

  const payload = {
    view_id: viewId,
    currenttime: duration,
    duration: duration,
    percent: 100,
    mapa: JSON.stringify(map),
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

// ── Site Info (Get User ID) ───────────────────────────────────────────────────

/**
 * Get site info including current user ID via pure WS API.
 */
export async function getSiteInfoApi(
  session: { wsToken: string; moodleBaseUrl: string }
): Promise<{ userid: number; username: string; fullname: string; sitename: string }> {
  const data = await moodleApiCall<any>(
    session,
    "core_webservice_get_site_info",
    {}
  );

  return {
    userid: data.userid,
    username: data.username,
    fullname: data.fullname,
    sitename: data.sitename,
  };
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
          isComplete: false, // API doesn't provide completion status
        });
      }
    }
  }

  return videos;
}

// ── Quizzes via WS API ────────────────────────────────────────────────────────

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

  return (data?.quizzes ?? []).map((q: any) => ({
    cmid: q.coursemodule.toString(),
    name: q.name,
    url: q.viewurl,
    isComplete: false, // API doesn't provide completion status
    timeOpen: q.timeopen,
    timeClose: q.timeclose,
    courseId: q.course,
  }));
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
