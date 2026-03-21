import type { Page } from "playwright-core";

// ── Core Types ────────────────────────────────────────────────────────

export interface AppConfig {
  courseUrl: string;
  moodleBaseUrl: string;
  headless: boolean;
  slowMo: number;
  authStatePath: string;
  ollamaModel?: string;
  ollamaBaseUrl: string;
}

export interface SessionInfo {
  sesskey: string;
  moodleBaseUrl: string;
  wsToken?: string;           // Moodle Web Service Token for API calls
  wsTokenExpires?: number;   // Token expiry timestamp (Unix epoch)
}

export interface Logger {
  info: (msg: string) => void;
  success: (msg: string) => void;
  warn: (msg: string) => void;
  error: (msg: string) => void;
  debug: (msg: string) => void;
}

// ── Course & Module Types ─────────────────────────────────────────────

export interface EnrolledCourse {
  id: number;
  fullname: string;
  shortname: string;
  idnumber?: string;
  category?: string;
  progress?: number;
  startdate?: number;
  enddate?: number;
}

export interface SuperVideoModule {
  cmid: string;
  name: string;
  url: string;
  isComplete: boolean;
}

export interface VideoActivity {
  name: string;
  url: string;
  viewId: number;
  duration: number;
  existingPercent: number;
}

export interface QuizModule {
  cmid: string;
  name: string;
  url: string;
  isComplete: boolean;
  timeOpen?: number;
  timeClose?: number;
}

// ── Forum Types ───────────────────────────────────────────────────────

export interface ForumModule {
  cmid: string;
  forumId: number;
  name: string;
  url: string;
  courseId: number;
  forumType: string; // 'general', 'news', 'social', etc.
}

export interface ForumPost {
  id: number;
  subject: string;
  author: string;
  authorId?: number;
  created: number;
  modified: number;
  message: string;
  discussionId: number;
  replies?: ForumPost[];
  unread?: boolean;
}

export interface ForumDiscussion {
  id: number;
  forumId: number;
  name: string;
  firstPostId: number;
  userId: number;
  userFullName: string;
  groupId?: number;
  timedue?: number;
  timeModified: number;
  timeStart?: number;
  timeEnd?: number;
  userModified?: number;
  userModifiedFullName?: string;
  postCount?: number; // numreplies
  unread?: boolean; // numunread > 0
  subject?: string;
  message?: string;
  pinned?: boolean;
  locked?: boolean;
  starred?: boolean;
}

// ── Announcement Types ────────────────────────────────────────────────

export interface AnnouncementPost {
  id: number;
  subject: string;
  content: string;
  author: string;
  authorId?: number;
  courseId: number;
  createdAt: number;
  modifiedAt?: number;
  attachments?: AnnouncementAttachment[];
  unread?: boolean;
}

export interface AnnouncementAttachment {
  filename: string;
  url: string;
  filesize: number;
  mimetype: string;
}

// ── Material/Resource Types ───────────────────────────────────────────

export interface ResourceModule {
  cmid: string;
  name: string;
  url: string;
  courseId: number;
  modType: string; // 'resource', 'url', 'folder', 'page', etc.
  mimetype?: string;
  filesize?: number;
  modified?: number;
}

export interface FolderContents {
  files: ResourceFile[];
  folders: FolderInfo[];
}

export interface ResourceFile {
  filename: string;
  url: string;
  filesize: number;
  mimetype: string;
  modified: number;
  path?: string; // for nested files in folders
}

export interface FolderInfo {
  name: string;
  id: number;
  path: string;
}

// ── Assignment Types ─────────────────────────────────────────────────

export interface AssignmentModule {
  cmid: string;
  name: string;
  url: string;
  courseId: number;
  duedate?: number;
  cutoffdate?: number;
  allowSubmissionsFromDate?: number;
  gradingduedate?: number;
  submissionStatus?: string;
  grade?: GradeInfo;
  lateSubmission?: boolean;
  extensionduedate?: number;
}

export interface AssignmentSubmission {
  id: number;
  assignmentId: number;
  userId: number;
  timemodified: number;
  status: string; // 'submitted', 'draft', 'new'
  files?: SubmissionFile[];
  grade?: GradeInfo;
  feedback?: string;
}

export interface SubmissionFile {
  filename: string;
  url: string;
  filesize: number;
  mimetype: string;
  timemodified: number;
}

export interface GradeInfo {
  grade: string;
  gradeFormatted: string;
  grader?: string;
  timedue?: number;
  timemodified?: number;
}

// ── Grade Types ──────────────────────────────────────────────────────

export interface CourseGrade {
  courseId: number;
  courseName: string;
  grade?: string;
  gradeFormatted?: string;
  rank?: number;
  totalUsers?: number;
  items?: GradeItem[];
}

export interface GradeItem {
  id: number;
  name: string;
  grade?: string;
  gradeFormatted?: string;
  range?: string; // e.g., "0-100"
  percentage?: number;
  weight?: number;
  feedback?: string;
  graded?: boolean;
}

// ── Calendar Types ───────────────────────────────────────────────────

export interface CalendarEvent {
  id: number;
  name: string;
  description?: string;
  format: number; // 0=None, 1=HTML, 2=Plain, 3=Markdown
  courseid?: number;
  categoryid?: number;
  groupid?: number;
  userid?: number;
  moduleid?: number;
  modulename?: string;
  instance?: number;
  eventtype: string; // 'user', 'group', 'course', 'site', 'due'
  timestart: number;
  timeduration?: number;
  timedue?: number;
  visible?: number;
  location?: string;
}

// ── LLM & Quiz Types ─────────────────────────────────────────────────

export interface QuestionParsing {
  id: string;
  text: string;
  options: { value: string; text: string }[];
  type: "radio" | "checkbox";
}

export interface QuestionAnswer {
  questionId: string;
  reasoning: string;
  answerValues: string[];
}

// ── Output Format Types ──────────────────────────────────────────────

export type OutputFormat = "json" | "csv" | "table" | "silent";

export interface OutputOptions {
  format: OutputFormat;
  fields?: string[]; // for CSV output
  pretty?: boolean; // for JSON output
}

// ── AJAX Types ───────────────────────────────────────────────────────

export interface ProgressPayload {
  view_id: number;
  currenttime: number;
  duration: number;
  percent: number;
  mapa: string;
}

export interface AjaxResponse {
  error: boolean;
  data?: { success?: boolean; exec?: string };
  exception?: { message: string; errorcode: string };
}

// ── CLI Command Types ────────────────────────────────────────────────

export interface CommandContext {
  page: Page;
  session: SessionInfo;
  config: AppConfig;
  log: Logger;
}

export interface CommandOptions {
  output?: OutputFormat;
  courseUrl?: string;
  courseId?: number;
  verbose?: boolean;
  headed?: boolean;
  dryRun?: boolean;
}
