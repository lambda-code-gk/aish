//! シェルログ解決のドメイン型と純粋検証。

use std::path::PathBuf;

pub const AI_ASK_LOG_SESSION: &str = "session";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellLogChoice {
    None,
    Path(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShellLogResolveError {
    #[error("AI_ASK_LOG must be \"session\" when set (got \"{0}\")")]
    InvalidAiAskLog(String),
    #[error("AI_ASK_LOG=session requires AISH_SESSION_DIR to be set and readable")]
    SessionDirRequired,
    #[error("--session requires AISH_SESSION_DIR to be set")]
    SessionDirRequiredForFlag,
    #[error("invalid session id: {0}")]
    InvalidSessionId(String),
    #[error("--session {id} does not match AISH_SESSION_DIR ({dir})")]
    SessionIdMismatch { id: String, dir: String },
    #[error("session log not found: {0}")]
    NotFound(String),
    #[error("session log unreadable: {0}: {1}")]
    Unreadable(String, String),
}

pub fn validate_session_id(id: &str) -> Result<(), ShellLogResolveError> {
    if id.len() != 12 {
        return Err(ShellLogResolveError::InvalidSessionId(id.to_string()));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ShellLogResolveError::InvalidSessionId(id.to_string()));
    }
    Ok(())
}
