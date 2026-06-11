//! contextual memory 永続化 port。

use aibe_protocol::MemoryOperationDto;

use crate::domain::{MemoryBlock, MemoryEntry, MemoryValidationError, ProjectKeyError};

#[derive(Debug, thiserror::Error)]
pub enum ContextualMemoryStoreError {
    #[error("validation: {0}")]
    Validation(#[from] MemoryValidationError),
    #[error("project key: {0}")]
    ProjectKey(#[from] ProjectKeyError),
    #[error("io: {0}")]
    Io(String),
    #[error("entry not found: {0}")]
    NotFound(String),
}

pub trait ContextualMemoryStore: Send + Sync {
    fn apply(
        &self,
        session_id: &str,
        cwd: Option<&std::path::Path>,
        operation: &MemoryOperationDto,
        now_ms: u64,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError>;

    fn query(
        &self,
        session_id: &str,
        cwd: Option<&std::path::Path>,
        query: &aibe_protocol::MemoryQueryDto,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError>;

    fn resolve_for_prompt(
        &self,
        session_id: &str,
        cwd: Option<&std::path::Path>,
        user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError>;
}
