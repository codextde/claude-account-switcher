use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Credentials(String),
    #[error("Claude CLI not found. Install Claude Code or set the path in Settings.")]
    CliNotFound,
    #[error("Claude CLI failed: {0}")]
    Cli(String),
    #[error("{0}")]
    State(String),
    #[error("Login cancelled")]
    LoginCancelled,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Tauri error: {0}")]
    Tauri(#[from] tauri::Error),
}

pub type Result<T> = std::result::Result<T, AppError>;

/// Commands return plain strings so the frontend can show them verbatim.
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
