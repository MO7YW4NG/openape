// ── Core Infrastructure ────────────────────────────────────────────────

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
  wsToken?: string;
  wsTokenExpires?: number;
}

export interface Logger {
  info: (msg: string) => void;
  success: (msg: string) => void;
  warn: (msg: string) => void;
  error: (msg: string) => void;
  debug: (msg: string) => void;
}

export type OutputFormat = "json" | "csv" | "table" | "silent";

export interface OutputOptions {
  format: OutputFormat;
  fields?: string[];
  pretty?: boolean;
}

// ── Course ────────────────────────────────────────────────────────────

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

// ── Video ─────────────────────────────────────────────────────────────

export interface SuperVideoModule {
  cmid: string;
  name: string;
  url: string;
  instance?: number;
  isComplete: boolean;
}

// ── Quiz ──────────────────────────────────────────────────────────────

export interface QuizModule {
  quizid: string;
  name: string;
  url: string;
  isComplete: boolean;
  attemptsUsed: number;
  maxAttempts: number;
  timeOpen?: number;
  timeClose?: number;
}

export interface QuizAttempt {
  attempt: number;
  attemptid: number;
  quizid: number;
  userid: number;
  attemptnumber: number;
  state: string;
  timestart: number;
  timefinish?: number;
  timemodified?: number;
  timecheckstate?: number;
  sumgrades?: number;
  layout?: string;
  uniqueid?: number;
  preview?: boolean;
}

export interface QuizQuestion {
  slot: number;
  type: string;
  typeid?: number;
  id: number;
  category?: number;
  contextid?: number;
  contextlevel?: string;
  contextinstanceid?: number;
  quizid: number;
  maxmark: number;
  minmark?: number;
  number?: number;
  page: number;
  html?: string;
  status?: string;
  stateclass?: string;
  sequencecheck?: number;
  questionnumber?: string;
}

export interface QuizAttemptData {
  attempt: QuizAttempt;
  questions: Record<number, QuizQuestion>;
  nextpage?: number;
  prevpage?: number;
  messages?: string[];
  quizflags?: Record<number, boolean>;
}

export interface QuizStartResult {
  attempt: QuizAttempt;
  page?: number;
  messages?: string[];
  readonly?: boolean;
}

// ── Forum ─────────────────────────────────────────────────────────────

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
  postCount?: number;
  unread?: boolean;
  subject?: string;
  message?: string;
  pinned?: boolean;
  locked?: boolean;
  starred?: boolean;
}

// ── Calendar ──────────────────────────────────────────────────────────

export interface CalendarEvent {
  id: number;
  name: string;
  description?: string;
  format: number;
  courseid?: number;
  categoryid?: number;
  groupid?: number;
  userid?: number;
  moduleid?: number;
  modulename?: string;
  instance?: number;
  eventtype: string;
  timestart: number;
  timeduration?: number;
  timedue?: number;
  visible?: number;
  location?: string;
}

// ── Grades ────────────────────────────────────────────────────────────

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
  range?: string;
  percentage?: number;
  weight?: number;
  feedback?: string;
  graded?: boolean;
}

// ── Materials ─────────────────────────────────────────────────────────

export interface ResourceModule {
  cmid: string;
  name: string;
  url: string;
  courseId: number;
  modType: string;
  mimetype?: string;
  filesize?: number;
  modified?: number;
}
