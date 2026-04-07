/// Moodle API error with context.
#[derive(Debug, thiserror::Error)]
pub enum MoodleError {
    #[error("WS API call failed: {function} — {message}")]
    WsApi { function: String, message: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
