//! Work state 永続化 port。

use crate::domain::{WorkMutationError, WorkState};

#[derive(Debug, Clone)]
pub struct WorkStoreContext {
    pub memory_space_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkStoreError {
    #[error("invalid memory space id")]
    InvalidMemorySpace,
    #[error("work state is corrupt: {0}")]
    Corrupt(String),
    #[error("work state validation failed: {0}")]
    Validation(String),
    #[error("work state I/O failed: {0}")]
    Io(String),
    #[error("work mutation failed: {0}")]
    Mutation(String),
    #[error("work operation rejected: {0}")]
    Operation(WorkMutationError),
}

pub trait WorkStore: Send + Sync {
    fn load(&self, ctx: &WorkStoreContext) -> Result<WorkState, WorkStoreError>;

    fn mutate(
        &self,
        ctx: &WorkStoreContext,
        mutation: &mut dyn FnMut(&mut WorkState) -> Result<(), WorkStoreError>,
    ) -> Result<WorkState, WorkStoreError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyWorkStore;

impl WorkStore for EmptyWorkStore {
    fn load(&self, _ctx: &WorkStoreContext) -> Result<WorkState, WorkStoreError> {
        Ok(WorkState::default())
    }

    fn mutate(
        &self,
        _ctx: &WorkStoreContext,
        _mutation: &mut dyn FnMut(&mut WorkState) -> Result<(), WorkStoreError>,
    ) -> Result<WorkState, WorkStoreError> {
        Err(WorkStoreError::Mutation(
            "work store is not configured".into(),
        ))
    }
}
