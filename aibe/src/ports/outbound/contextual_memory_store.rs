//! contextual memory 永続化 port。

use std::path::Path;

use aibe_protocol::MemoryOperationDto;

use crate::domain::{MemoryBlock, MemoryEntry, MemoryValidationError, ProjectKeyError};

#[derive(Debug, Clone)]
pub struct MemoryStoreContext<'a> {
    pub session_id: &'a str,
    pub memory_space_id: String,
    pub cwd: Option<&'a Path>,
}

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
    #[error("kind registry: {0}")]
    Registry(#[from] crate::domain::MemoryKindRegistryError),
}

pub trait ContextualMemoryStore: Send + Sync {
    fn apply(
        &self,
        ctx: &MemoryStoreContext<'_>,
        operation: &MemoryOperationDto,
        now_ms: u64,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError>;

    fn query(
        &self,
        ctx: &MemoryStoreContext<'_>,
        query: &aibe_protocol::MemoryQueryDto,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError>;

    fn resolve_for_prompt(
        &self,
        ctx: &MemoryStoreContext<'_>,
        user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError>;

    /// explicit memory RPC 用。registry parse 失敗時は error（AgentTurn は `resolve_for_prompt` の best-effort を使う）。
    fn resolve_for_prompt_explicit(
        &self,
        ctx: &MemoryStoreContext<'_>,
        user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError>;
}
