use crate::domain::human_task_checkpoint::{HumanTaskCheckpointV1, HumanTaskId};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum HumanTaskStoreError {
    #[error("human_task_checkpoint_not_found")]
    NotFound,
    #[error("human_task_checkpoint_invalid")]
    Invalid,
    #[error("human_task_checkpoint_version_unsupported")]
    VersionUnsupported,
    #[error("human_task_checkpoint_permission_denied")]
    PermissionDenied,
    #[error("human_task_checkpoint_unavailable")]
    Unavailable,
}

pub trait HumanTaskStoreLock: Send {}

pub trait HumanTaskStore: Send + Sync {
    fn lock_exclusive(&self) -> Result<Box<dyn HumanTaskStoreLock + '_>, HumanTaskStoreError>;
    fn load_active(&self) -> Result<HumanTaskCheckpointV1, HumanTaskStoreError>;
    fn save(&self, checkpoint: &HumanTaskCheckpointV1) -> Result<(), HumanTaskStoreError>;
    fn remove(&self, task_id: &HumanTaskId) -> Result<(), HumanTaskStoreError>;
}
pub trait HumanTaskIdentity: Send + Sync {
    fn new_task_id(&self) -> HumanTaskId;
    fn now_ms(&self) -> u64;
}

pub trait HumanTaskTimeFormatter: Send + Sync {
    fn format_local(&self, timestamp_ms: u64) -> String;
}
