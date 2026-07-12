use crate::domain::CollabOutcome;

#[derive(Debug, thiserror::Error)]
pub enum CollabOutcomeCollectionError {
    #[error("Cannot collect Collaborative Mode result because stdin is not interactive.")]
    NonInteractiveStdin,
    #[error("Collaborative Mode result input ended before a result was provided.")]
    UnexpectedEof,
    #[error("Collaborative Mode result I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub trait CollabOutcomeCollector {
    fn collect(&self) -> Result<CollabOutcome, CollabOutcomeCollectionError>;
}
