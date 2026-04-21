pub mod announcements;
pub mod pages;
pub mod assignments;
pub mod auth;
pub mod calendar;
pub mod courses;
pub mod forums;
pub mod grades;
pub mod materials;
pub mod quizzes;
pub mod skills;
pub mod upload;
pub mod videos;

use crate::config::load_config_for_cli;
use crate::logger::Logger;
use crate::moodle::types::SessionInfo;
use crate::OutputFormat;

/// Shared context for API-only commands (no browser needed).
pub struct ApiCtx {
    pub client: reqwest::Client,
    pub session: SessionInfo,
    pub log: Logger,
    pub output: OutputFormat,
}

impl ApiCtx {
    pub fn build(cli: &crate::Cli) -> anyhow::Result<Self> {
        let config = load_config_for_cli(cli);
        let log = Logger::new(cli.verbose, cli.silent);
        let session = crate::auth::create_api_context(&config, &log)?;
        let mut builder = reqwest::Client::builder();
        if let Some(ref ua) = session.user_agent {
            let full_ua = format!("{ua} SEB/3.8");
            log.debug(&format!("Using User-Agent: {}", full_ua));
            builder = builder.user_agent(full_ua);
        }
        if let Some(cookie_header) = crate::auth::load_cookie_header(&config.auth_state_path, &config.moodle_base_url) {
            log.debug("Injecting saved session cookies into HTTP client.");
            let mut headers = reqwest::header::HeaderMap::new();
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&cookie_header) {
                headers.insert(reqwest::header::COOKIE, val);
                builder = builder.default_headers(headers);
            }
        }
        let client = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;
        Ok(ApiCtx { client, session, log, output: cli.output })
    }
}

/// Map CourseLevel CLI enum to Moodle classification string.
pub fn level_to_classification(level: crate::CourseLevel) -> &'static str {
    match level {
        crate::CourseLevel::InProgress => "inprogress",
        crate::CourseLevel::Past => "past",
        crate::CourseLevel::Future => "future",
        crate::CourseLevel::All => "all",
    }
}

/// Map in-progress/all CLI enum to Moodle classification string.
pub fn in_progress_all_to_classification(level: crate::InProgressAllLevel) -> &'static str {
    match level {
        crate::InProgressAllLevel::InProgress => "inprogress",
        crate::InProgressAllLevel::All => "all",
    }
}
