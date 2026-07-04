//! rollback journal outbound port（設計 §19）。

use std::path::PathBuf;

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{BeforeState, FileChangeOperation};

/// journal 保存要求。
#[derive(Debug, Clone)]
pub struct JournalSaveRequest {
    pub tool: String,
    pub target_path: PathBuf,
    pub before_state: BeforeState,
    pub before_bytes: Option<Vec<u8>>,
    pub before_sha256: Option<String>,
    pub after_sha256: String,
    pub after_bytes: usize,
    pub file_mode: Option<u32>,
    pub operation: FileChangeOperation,
}

/// 保存済み journal エントリ。
#[derive(Debug, Clone)]
pub struct JournalEntry {
    pub change_id: String,
    pub dir: PathBuf,
}

/// journal エラー（設計 §21）。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FileChangeJournalError {
    #[error("journal_failed")]
    Failed,
    #[error("journal_capacity_exceeded")]
    CapacityExceeded,
}

#[async_trait]
pub trait FileChangeJournal: Send + Sync {
    async fn save_before(
        &self,
        request: JournalSaveRequest,
    ) -> Result<JournalEntry, FileChangeJournalError>;

    async fn cleanup_expired(&self) -> Result<(), FileChangeJournalError>;

    async fn mark_status(
        &self,
        entry: &JournalEntry,
        status: &str,
    ) -> Result<(), FileChangeJournalError>;
}
