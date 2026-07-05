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
pub struct LeaseHeartbeatRequest {
    pub owner_client_id: String,
    pub owner_process_id: u32,
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
    fn list_handoffs(&self) -> Result<Vec<Handoff>, HandoffStoreError>;
}

pub trait LeaseRepository {
    fn try_acquire_lease(
        &self,
        handoff_id: &str,
        request: &LeaseAcquireRequest,
    ) -> Result<HandoffLease, HandoffStoreError>;

    fn load_lease(&self, handoff_id: &str) -> Result<Option<HandoffLease>, HandoffStoreError>;

    /// owner が一致する lease を原子的に延長する。失効済み lease は延長しない。
    fn heartbeat_lease(
        &self,
        handoff_id: &str,
        request: &LeaseHeartbeatRequest,
    ) -> Result<HandoffLease, HandoffStoreError>;

    /// 正常に side run が終了・人間待ちへ遷移した時の lease 解放。
    fn release_lease(&self, _handoff_id: &str) -> Result<(), HandoffStoreError> {
        Ok(())
    }
}

/// Human-shell lifetime lease とは独立した、side agent 1 run の排他境界。
pub trait SideRunLockRepository {
    fn try_acquire_side_run_lock(
        &self,
        handoff_id: &str,
        request: &LeaseAcquireRequest,
    ) -> Result<HandoffLease, HandoffStoreError>;

    fn release_side_run_lock(&self, handoff_id: &str) -> Result<(), HandoffStoreError>;

    fn load_side_run_lock(
        &self,
        handoff_id: &str,
    ) -> Result<Option<HandoffLease>, HandoffStoreError>;

    /// handoff 単位ロック下で stale side-run lock を解放し、呼び出し側が state を更新する。
    fn recover_stale_side_agent_run(
        &self,
        handoff_id: &str,
        owner_is_alive: &dyn Fn(u32) -> bool,
        now_ms: u64,
        update: &mut dyn FnMut(
            &mut Handoff,
            &mut HandoffCheckpoint,
        ) -> Result<(), HandoffStoreError>,
    ) -> Result<bool, HandoffStoreError>;

    /// handoff / checkpoint 更新と side-run lock 削除を同一 store lock 内で行う。
    /// `resume_tool_call_id` は `request_human_action` 再開時の tool lifecycle 確定用。
    fn finish_side_run_atomically(
        &self,
        handoff_id: &str,
        now_ms: u64,
        candidates: &[CommandCandidate],
        resume_tool_call_id: Option<&str>,
        update: &mut dyn FnMut(
            &mut Handoff,
            &mut HandoffCheckpoint,
        ) -> Result<(), HandoffStoreError>,
    ) -> Result<(), HandoffStoreError>;
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

pub trait HandoffAuditRepository {
    fn record_audit(
        &self,
        handoff_id: &str,
        kind: crate::domain::CollaborativeAuditKind,
    ) -> Result<(), HandoffStoreError>;
}
