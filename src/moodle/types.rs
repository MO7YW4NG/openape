use serde::{Deserialize, Serialize};

// ── Infrastructure ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub moodle_base_url: String,
    pub ws_token: Option<String>,
    pub user_agent: Option<String>,
}

// ── Course ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrolledCourse {
    pub id: u64,
    pub fullname: String,
    pub shortname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idnumber: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startdate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enddate: Option<i64>,
}

// ── Video ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperVideoModule {
    pub cmid: String,
    pub name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<u64>,
    pub is_complete: bool,
}

// ── Quiz ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizModule {
    pub quizid: String,
    pub name: String,
    pub url: String,
    pub is_complete: bool,
    pub attempts_used: u32,
    pub max_attempts: u32,
    pub intro: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_open: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_close: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizAttempt {
    pub attempt: u64,
    pub attemptid: u64,
    pub quizid: u64,
    pub userid: u64,
    pub attemptnumber: u32,
    pub state: String,
    pub timestart: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timefinish: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniqueid: Option<u64>,
    pub preview: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizQuestion {
    pub slot: u32,
    #[serde(rename = "type")]
    pub qtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub maxmark: f64,
    pub page: u32,
    pub quizid: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stateclass: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequencecheck: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub questionnumber: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_answer: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QuizAttemptData {
    pub attempt: QuizAttempt,
    pub questions: std::collections::HashMap<u32, QuizQuestion>,
    pub nextpage: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct QuizStartResult {
    pub attempt: QuizAttempt,
    pub messages: Option<Vec<String>>,
}

// ── Forum ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumPost {
    pub id: u64,
    pub subject: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<u64>,
    pub created: i64,
    pub modified: i64,
    pub message: String,
    pub discussion_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumDiscussion {
    pub id: u64,
    pub forum_id: u64,
    pub name: String,
    pub first_post_id: u64,
    pub user_id: u64,
    pub user_full_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_due: Option<i64>,
    pub time_modified: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
}

// ── Calendar ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub format: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub courseid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categoryid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groupid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moduleid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modulename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<u64>,
    pub eventtype: String,
    pub timestart: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeduration: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timedue: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

// ── Grades ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CourseGrade {
    pub course_id: u64,
    pub course_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_formatted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_users: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<GradeItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradeItem {
    pub id: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_formatted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    pub graded: bool,
}

// ── Materials ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceModule {
    pub cmid: String,
    pub name: String,
    pub url: String,
    pub course_id: u64,
    pub mod_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mimetype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesize: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
}

// ── Assignments ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentModule {
    pub id: u64,
    pub cmid: String,
    pub name: String,
    pub url: String,
    pub course_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duedate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutoffdate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_submissions_from_date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grading_due_date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub late_submission: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_due_date: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionStatus {
    pub submitted: bool,
    pub graded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<i64>,
    pub extensions: Vec<DraftFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftFile {
    pub id: u64,
    pub filename: String,
    pub filesize: u64,
}

// ── Messages ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: u64,
    pub useridfrom: u64,
    pub useridto: u64,
    pub subject: String,
    pub text: String,
    pub timecreated: i64,
}

// ── Pages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageModule {
    pub id: u64,
    pub cmid: String,
    pub name: String,
    pub course_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timemodified: Option<i64>,
}

// ── Site Info ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SiteInfo {
    pub userid: u64,
}
