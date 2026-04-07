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

use crate::config::load_config;
use crate::logger::Logger;
use crate::moodle::types::SessionInfo;
use crate::OutputFormat;
use std::path::PathBuf;

/// Shared context for API-only commands (no browser needed).
pub struct ApiCtx {
    pub client: reqwest::Client,
    pub session: SessionInfo,
    pub log: Logger,
    pub output: OutputFormat,
}

impl ApiCtx {
    pub fn build(config_path: Option<&PathBuf>, output: OutputFormat, verbose: bool, silent: bool) -> anyhow::Result<Self> {
        let config = load_config(config_path.and_then(|p| p.parent()));
        let log = Logger::new(verbose, silent);
        let session = crate::auth::create_api_context(&config, &log)?;
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;
        Ok(ApiCtx { client, session, log, output })
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
