//! local history storage outbound port.

use crate::domain::{HistoryIndexEntry, HistoryPayload};

#[derive(Debug, thiserror::Error)]
pub enum HistoryStoreError {
    #[error("failed to read history: {0}")]
    Read(String),
    #[error("failed to write history: {0}")]
    Write(String),
    #[error("history entry not found: {0}")]
    NotFound(String),
}

pub trait HistoryStore {
    fn append(
        &self,
        entry: &HistoryIndexEntry,
        payload: &HistoryPayload,
    ) -> Result<(), HistoryStoreError>;
    fn list(&self) -> Result<Vec<HistoryIndexEntry>, HistoryStoreError>;
    fn load_payload(&self, history_id: &str) -> Result<HistoryPayload, HistoryStoreError>;
    /// Keep the newest `max_entries` rows; delete older payloads. Returns removed count.
    fn prune_to_max(&self, max_entries: usize) -> Result<usize, HistoryStoreError>;
}
