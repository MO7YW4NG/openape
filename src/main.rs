use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

mod auth;
mod commands;
mod config;
mod error;
mod logger;
mod moodle;
mod output;
mod utils;

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Json,
    Csv,
    Table,
    Silent,
}

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum CourseLevel {
    #[default]
    #[value(name = "in_progress")]
    InProgress,
    Past,
    Future,
    All,
}

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum InProgressAllLevel {
    #[default]
    #[value(name = "in_progress")]
    InProgress,
    #[value(name = "all")]
    All,
}

#[derive(Parser)]
#[command(name = "openape", version, about = "CLI tool for CYCU i-Learning platform")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[arg(long, global = true, default_value = ".auth/storage-state.json")]
    session: PathBuf,

    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::default())]
    output: OutputFormat,

    #[arg(long, global = true)]
    verbose: bool,

    #[arg(long, global = true)]
    silent: bool,

    #[arg(long, global = true)]
    headed: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Login to iLearning manually and save session
    Login,
    /// Check session status
    Status,
    /// Remove saved session
    Logout,
    #[command(subcommand)]
    Courses(CoursesCommands),
    #[command(subcommand)]
    Videos(VideosCommands),
    #[command(subcommand)]
    Quizzes(QuizzesCommands),
    #[command(subcommand)]
    Materials(MaterialsCommands),
    #[command(subcommand)]
    Grades(GradesCommands),
    #[command(subcommand)]
    Forums(ForumsCommands),
    #[command(subcommand)]
    Announcements(AnnouncementsCommands),
    #[command(subcommand)]
    Calendar(CalendarCommands),
    #[command(subcommand)]
    Assignments(AssignmentsCommands),
    #[command(subcommand)]
    Pages(PagesCommands),
    #[command(subcommand)]
    Upload(UploadCommands),
    #[command(subcommand)]
    Skills(SkillsCommands),
}

// ── Auth (top-level commands) ─────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Login to iLearning manually and save session
    Login,
    /// Check session status
    Status,
    /// Remove saved session
    Logout,
}

// ── Courses ───────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum CoursesCommands {
    /// List enrolled courses
    List {
        #[arg(long)]
        incomplete_only: bool,
        #[arg(long, value_enum, default_value_t = CourseLevel::default())]
        level: CourseLevel,
    },
    /// Show detailed course information
    Info {
        course_id: u64,
    },
    /// Show course progress
    Progress {
        course_id: u64,
    },
    /// Show course syllabus (from CMAP)
    Syllabus {
        course_id: u64,
    },
}

// ── Videos ────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum VideosCommands {
    /// List videos in a course
    List {
        course_id: u64,
        #[arg(long)]
        incomplete_only: bool,
    },
    /// Complete videos in a course
    Complete {
        course_id: u64,
        #[arg(long)]
        dry_run: bool,
    },
    /// Complete all incomplete videos across all courses
    CompleteAll {
        #[arg(long)]
        dry_run: bool,
    },
    /// Download videos from a course
    Download {
        course_id: u64,
        #[arg(long, default_value = "./downloads/videos")]
        output_dir: PathBuf,
        #[arg(long)]
        incomplete_only: bool,
    },
}

// ── Quizzes ───────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum QuizzesCommands {
    /// List quizzes in a course
    List {
        course_id: u64,
        #[arg(long)]
        all: bool,
    },
    /// List all quizzes across all courses
    ListAll {
        #[arg(long, value_enum, default_value_t = InProgressAllLevel::default())]
        level: InProgressAllLevel,
        #[arg(long)]
        all: bool,
    },
    /// Start a new quiz attempt
    Start {
        quiz_id: u64,
    },
    /// Get quiz attempt data and questions
    Info {
        attempt_id: u64,
        #[arg(long, default_value = "-1")]
        page: i32,
    },
    /// Save answers for a quiz attempt
    Save {
        attempt_id: u64,
        #[arg(value_name = "ANSWERS_JSON_OR_DELIMITED")]
        answers: String,
        #[arg(long)]
        submit: bool,
    },
}

// ── Materials ─────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum MaterialsCommands {
    /// List all materials across all courses
    ListAll {
        #[arg(long, value_enum, default_value_t = InProgressAllLevel::default())]
        level: InProgressAllLevel,
    },
    /// Download all materials from a specific course
    Download {
        course_id: u64,
        #[arg(long, default_value = "./downloads")]
        output_dir: PathBuf,
    },
    /// Download all materials from all courses
    DownloadAll {
        #[arg(long, default_value = "./downloads")]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = CourseLevel::default())]
        level: CourseLevel,
    },
    /// Mark all incomplete resources as complete in a course
    Complete {
        course_id: u64,
        #[arg(long)]
        dry_run: bool,
    },
    /// Mark all incomplete resources as complete across all in-progress courses
    CompleteAll {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, value_enum, default_value_t = CourseLevel::default())]
        level: CourseLevel,
    },
}

// ── Grades ────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum GradesCommands {
    /// Show grade summary across all courses
    Summary,
    /// Show detailed grades for a specific course
    Course {
        course_id: u64,
    },
}

// ── Forums ────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum ForumsCommands {
    /// List forums from in-progress courses
    List,
    /// List all forums across all courses
    ListAll {
        #[arg(long, value_enum, default_value_t = InProgressAllLevel::default())]
        level: InProgressAllLevel,
    },
    /// List discussions in a forum
    Discussions {
        forum_id: u64,
    },
    /// Show posts in a discussion
    Posts {
        discussion_id: u64,
    },
    /// Post a new discussion to a forum
    Post {
        forum_id: u64,
        subject: String,
        message: String,
        #[arg(long, default_value_t = false)]
        subscribe: bool,
        #[arg(long, default_value_t = false)]
        pin: bool,
    },
    /// Reply to a discussion post
    Reply {
        post_id: u64,
        subject: String,
        message: String,
        #[arg(long)]
        attachment_id: Option<u64>,
        #[arg(long)]
        inline_attachment_id: Option<u64>,
    },
    /// Delete a forum post or discussion
    Delete {
        post_id: u64,
    },
}

// ── Announcements ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AnnouncementsCommands {
    /// List all announcements across all courses
    ListAll {
        #[arg(long)]
        unread_only: bool,
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Read a specific announcement
    Read {
        announcement_id: u64,
    },
}

// ── Calendar ──────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum CalendarCommands {
    /// List calendar events
    Events {
        #[arg(long)]
        upcoming: bool,
        #[arg(long, default_value = "30")]
        days: u32,
        #[arg(long)]
        course: Option<u64>,
    },
    /// Export calendar events to file
    Export {
        #[arg(long, default_value = "./calendar.json")]
        output: PathBuf,
        #[arg(long, default_value = "30")]
        days: u32,
    },
}

// ── Assignments ───────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AssignmentsCommands {
    /// List assignments in a course
    List {
        course_id: u64,
    },
    /// List all assignments across all courses
    ListAll {
        #[arg(long, value_enum, default_value_t = InProgressAllLevel::default())]
        level: InProgressAllLevel,
    },
    /// Check assignment submission status
    Status {
        assignment_id: u64,
    },
    /// Submit an assignment (online text or file)
    Submit {
        assignment_id: u64,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        file_id: Option<u64>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

// ── Upload ────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum UploadCommands {
    /// Upload a file to Moodle draft area
    File {
        file_path: PathBuf,
        #[arg(long)]
        filename: Option<String>,
    },
}

// ── Pages ─────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum PagesCommands {
    /// List pages in a course
    List {
        course_id: u64,
    },
    /// List all pages across all courses
    ListAll {
        #[arg(long, value_enum, default_value_t = InProgressAllLevel::default())]
        level: InProgressAllLevel,
    },
    /// Show the content of a specific page
    Show {
        cmid: u64,
    },
}

// ── Skills ────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SkillsCommands {
    /// Install the OpenApe skill to an agent platform
    Install {
        /// Agent platform (claude, codex, opencode)
        platform: Option<String>,
        /// Detect installed agents and install to all
        #[arg(long)]
        all: bool,
    },
    /// Print the raw SKILL.md content
    Show,
}

// ── Main ──────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    rt.block_on(async {
        if let Err(e) = run_command(cli).await {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
    });
}

async fn run_command(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Login => commands::auth::run(&AuthCommands::Login, &cli).await,
        Commands::Status => commands::auth::run(&AuthCommands::Status, &cli).await,
        Commands::Logout => commands::auth::run(&AuthCommands::Logout, &cli).await,
        Commands::Courses(ref cmd) => commands::courses::run(cmd, &cli).await,
        Commands::Videos(ref cmd) => commands::videos::run(cmd, &cli).await,
        Commands::Quizzes(ref cmd) => commands::quizzes::run(cmd, &cli).await,
        Commands::Materials(ref cmd) => commands::materials::run(cmd, &cli).await,
        Commands::Grades(ref cmd) => commands::grades::run(cmd, &cli).await,
        Commands::Forums(ref cmd) => commands::forums::run(cmd, &cli).await,
        Commands::Announcements(ref cmd) => commands::announcements::run(cmd, &cli).await,
        Commands::Calendar(ref cmd) => commands::calendar::run(cmd, &cli).await,
        Commands::Assignments(ref cmd) => commands::assignments::run(cmd, &cli).await,
        Commands::Upload(ref cmd) => commands::upload::run(cmd, &cli).await,
        Commands::Pages(ref cmd) => commands::pages::run(cmd, &cli).await,
        Commands::Skills(ref cmd) => commands::skills::run(cmd, &cli).await,
    }
}
