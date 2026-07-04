//! handoff / lease / checkpoint 永続化 outbound port（0055）。

use crate::domain::{
    CommandCandidate, Handoff, HandoffCheckpoint, HandoffLease, HandoffShellSession,
};

#[derive(Debug, Clone)]
pub struct LeaseAcquireRequest {
    pub owner_client_id: String,
    pub owner_process_id: u32,
    pub owner_tty: Option<String>,
    pub owner_host: String,
    pub owner_uid: u32,
    pub now_ms: u64,
    pub lease_timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ShellSessionIssueRequest {
    pub generation: u32,
    pub token_plaintext: String,
    pub now_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum HandoffStoreError {
    #[error("failed to read handoff store: {0}")]
    Read(String),
    #[error("failed to write handoff store: {0}")]
    Write(String),
    #[error("handoff not found: {0}")]
    NotFound(String),
    #[error("lease already held by another owner")]
    LeaseConflict,
    #[error("invalid handoff id")]
    InvalidHandoffId,
    #[error("invalid shell session generation")]
    InvalidShellGeneration,
}

pub trait HandoffRepository {
    fn save_handoff(&self, handoff: &Handoff) -> Result<(), HandoffStoreError>;
    fn load_handoff(&self, handoff_id: &str) -> Result<Handoff, HandoffStoreError>;
}

pub trait LeaseRepository {
    fn try_acquire_lease(
        &self,
        handoff_id: &str,
        request: &LeaseAcquireRequest,
    ) -> Result<HandoffLease, HandoffStoreError>;

    fn load_lease(&self, handoff_id: &str) -> Result<Option<HandoffLease>, HandoffStoreError>;
}

pub trait CheckpointRepository {
    fn save_checkpoint(
        &self,
        handoff_id: &str,
        checkpoint: &HandoffCheckpoint,
    ) -> Result<(), HandoffStoreError>;

    fn load_checkpoint(&self, handoff_id: &str) -> Result<HandoffCheckpoint, HandoffStoreError>;
}

pub trait HandoffShellSessionStore {
    fn append_shell_session(
        &self,
        handoff_id: &str,
        request: &ShellSessionIssueRequest,
    ) -> Result<HandoffShellSession, HandoffStoreError>;

    fn list_shell_sessions(
        &self,
        handoff_id: &str,
    ) -> Result<Vec<HandoffShellSession>, HandoffStoreError>;
}

pub trait CommandCandidateStore {
    fn append_candidate(
        &self,
        handoff_id: &str,
        candidate: &CommandCandidate,
    ) -> Result<(), HandoffStoreError>;

    fn list_candidates(&self, handoff_id: &str)
        -> Result<Vec<CommandCandidate>, HandoffStoreError>;
}
