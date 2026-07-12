//! Collaborative Mode の明示的な作業結果。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollabOutcomeStatus {
    Done,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollabOutcome {
    pub status: CollabOutcomeStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("status must be d/done, b/blocked, or c/cancelled")]
pub struct ParseCollabOutcomeStatusError;

pub fn parse_collab_outcome_status(
    input: &str,
) -> Result<CollabOutcomeStatus, ParseCollabOutcomeStatusError> {
    match input.trim().to_ascii_lowercase().as_str() {
        "d" | "done" => Ok(CollabOutcomeStatus::Done),
        "b" | "blocked" => Ok(CollabOutcomeStatus::Blocked),
        "c" | "cancelled" => Ok(CollabOutcomeStatus::Cancelled),
        _ => Err(ParseCollabOutcomeStatusError),
    }
}

impl CollabOutcome {
    pub fn new(status: CollabOutcomeStatus) -> Self {
        Self { status }
    }
}
