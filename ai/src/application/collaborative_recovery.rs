//! Collaborative handoff のクラッシュ復旧（0055 Phase 4）。

use std::path::PathBuf;

use super::{
    close_child_goal_durable, compensate_child_goal_durable,
    reconcile_child_goal_before_parent_resume, reconcile_incomplete_creating_handoff,
};
use crate::domain::{
    child_goal_close_blocks_parent_resume, collect_unknown_tool_ids, collect_unknown_tools,
    mark_uncertain_tools_on_disconnect, try_transition, ChildGoalCloseReason,
    CollaborativeAuditKind, Handoff, HandoffCheckpoint, HandoffEvent, HandoffState,
    RecoverableToolStatus, RequestedShellExec,
};
use crate::ports::outbound::{
    CheckpointRepository, CollaborativeChildGoalService, EnvironmentObserver,
    HandoffAuditRepository, HandoffRepository, HandoffRuntime, HandoffShellSessionStore,
    HandoffStoreError, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn, LeaseAcquireRequest, LeaseRepository, ShellSessionIssueRequest,
};

const LEASE_TIMEOUT_MS: u64 = 120_000;

pub trait RecoveryStore:
    HandoffRepository
    + CheckpointRepository
    + HandoffShellSessionStore
    + LeaseRepository
    + HandoffAuditRepository
{
}

impl<T> RecoveryStore for T where
    T: HandoffRepository
        + CheckpointRepository
        + HandoffShellSessionStore
        + LeaseRepository
        + HandoffAuditRepository
{
}

fn record_audit<S: HandoffAuditRepository>(
    store: &S,
    handoff_id: &str,
    kind: CollaborativeAuditKind,
) {
    let _ = store.record_audit(handoff_id, kind);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryOwner {
    pub client_id: String,
    pub process_id: u32,
    pub tty: Option<String>,
}

impl RecoveryOwner {
    pub fn from_runtime<R: HandoffRuntime>(runtime: &R) -> Self {
        Self {
            client_id: format!("ai-resume-{}", runtime.process_id()),
            process_id: runtime.process_id(),
            tty: runtime.tty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverableHandoffSummary {
    pub handoff_id: String,
    pub parent_task: String,
    pub child_goal_id: String,
    pub state: HandoffState,
    pub cwd: String,
    pub updated_at_ms: u64,
    pub lease_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentResumeContext {
    pub handoff_id: String,
    pub parent_task_id: String,
    pub parent_conversation_id: String,
    pub parent_goal: String,
    pub pending_shell_exec: RequestedShellExec,
    pub pending_human_request: Option<String>,
    pub conversation_summary: String,
    pub cwd: PathBuf,
    pub uncertain_tool_executions: Vec<String>,
    pub uncertain_tool_details: Vec<crate::domain::RecoverableToolExecution>,
    pub resume_observation: Option<String>,
}

impl ParentResumeContext {
    /// provider の未完了 tool-call ID へ再接続せず、新しい親 run に渡す意味的入力。
    pub fn semantic_prompt(&self) -> String {
        let unknown_tools: Vec<_> = self
            .uncertain_tool_details
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "tool_call_id": tool.tool_call_id,
                    "tool_name": tool.tool_name,
                    "status": "UNKNOWN",
                    "result": "uncertain",
                    "instructions": [
                        "Do not automatically re-run this tool.",
                        "Verify the current environment state before continuing."
                    ]
                })
            })
            .collect();
        serde_json::json!({
            "event": "collaborative_handoff_parent_resume",
            "handoff_id": self.handoff_id,
            "parent_goal": self.parent_goal,
            "pending_shell_exec": self.pending_shell_exec,
            "pending_human_request": self.pending_human_request,
            "conversation_summary": self.conversation_summary,
            "cwd": self.cwd,
            "uncertain_tool_executions": self.uncertain_tool_executions,
            "uncertain_tools": unknown_tools,
            "resume_observation": self.resume_observation,
            "instructions": [
                "Control returned from the human shell.",
                "Do not infer that the requested command ran or succeeded.",
                "Re-observe the environment before continuing.",
                "Do not automatically re-run UNKNOWN tool executions."
            ]
        })
        .to_string()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CollaborativeRecoveryError {
    #[error(transparent)]
    Store(#[from] HandoffStoreError),
    #[error(transparent)]
    Launch(#[from] HumanShellLaunchError),
    #[error("handoff {handoff_id} in state {state:?} cannot be resumed this way")]
    InvalidResumeState {
        handoff_id: String,
        state: HandoffState,
    },
    #[error("handoff state transition failed: {0}")]
    Transition(String),
    #[error("failed to generate secure handoff token: {0}")]
    Token(String),
    #[error("multiple recoverable handoffs exist; specify a handoff ID")]
    HandoffIdRequired(Vec<RecoverableHandoffSummary>),
    #[error("no recoverable handoff exists")]
    NoRecoverableHandoff,
}

pub fn list_recoverable_handoffs<S: HandoffRepository + LeaseRepository>(
    store: &S,
    now_ms: u64,
) -> Result<Vec<RecoverableHandoffSummary>, CollaborativeRecoveryError> {
    let mut out = Vec::new();
    for handoff in store.list_handoffs()? {
        if !matches!(
            handoff.state,
            HandoffState::Orphaned | HandoffState::Returned
        ) {
            continue;
        }
        let lease_active = store
            .load_lease(&handoff.id)?
            .is_some_and(|lease| lease.lease_expires_at_ms > now_ms);
        out.push(RecoverableHandoffSummary {
            handoff_id: handoff.id,
            parent_task: handoff.parent_request_summary,
            child_goal_id: handoff.child_goal_id,
            state: handoff.state,
            cwd: handoff.initial_cwd,
            updated_at_ms: handoff.updated_at_ms,
            lease_active,
        });
    }
    Ok(out)
}

pub fn select_recoverable_handoff<S: HandoffRepository + LeaseRepository>(
    store: &S,
    requested_id: Option<&str>,
    now_ms: u64,
) -> Result<String, CollaborativeRecoveryError> {
    if let Some(id) = requested_id {
        let handoff = store.load_handoff(id)?;
        if !matches!(
            handoff.state,
            HandoffState::Orphaned | HandoffState::Returned
        ) {
            return Err(invalid_state(&handoff));
        }
        return Ok(id.to_string());
    }
    let recoverable = list_recoverable_handoffs(store, now_ms)?;
    match recoverable.as_slice() {
        [] => Err(CollaborativeRecoveryError::NoRecoverableHandoff),
        [one] => Ok(one.handoff_id.clone()),
        _ => Err(CollaborativeRecoveryError::HandoffIdRequired(recoverable)),
    }
}

pub struct MarkOrphaned<'a, S, R> {
    store: &'a S,
    runtime: &'a R,
}

impl<'a, S: RecoveryStore, R: HandoffRuntime> MarkOrphaned<'a, S, R> {
    pub fn new(store: &'a S, runtime: &'a R) -> Self {
        Self { store, runtime }
    }

    pub fn execute(
        &self,
        handoff_id: &str,
        reason: &str,
    ) -> Result<(), CollaborativeRecoveryError> {
        let mut handoff = self.store.load_handoff(handoff_id)?;
        handoff.state = transition(handoff.state, HandoffEvent::Orphaned)?;
        handoff.return_reason = Some(reason.to_string());
        handoff.updated_at_ms = self.runtime.now_ms();
        let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
        mark_uncertain_tools_on_disconnect(&mut checkpoint);
        checkpoint.control_state = HandoffState::Orphaned;
        self.store.save_checkpoint(handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        self.store.release_lease(handoff_id)?;
        Ok(())
    }
}

pub struct CancelHandoff<'a, S, R> {
    store: &'a S,
    runtime: &'a R,
    child_goal: &'a dyn CollaborativeChildGoalService,
}

impl<'a, S: RecoveryStore, R: HandoffRuntime> CancelHandoff<'a, S, R> {
    pub fn new(
        store: &'a S,
        runtime: &'a R,
        child_goal: &'a dyn CollaborativeChildGoalService,
    ) -> Self {
        Self {
            store,
            runtime,
            child_goal,
        }
    }

    pub fn execute(
        &self,
        handoff_id: &str,
        reason: &str,
    ) -> Result<(), CollaborativeRecoveryError> {
        let mut handoff = self.store.load_handoff(handoff_id)?;
        handoff.state = transition(handoff.state, HandoffEvent::Cancel)?;
        handoff.return_reason = Some(reason.to_string());
        handoff.updated_at_ms = self.runtime.now_ms();
        if let Ok(mut checkpoint) = self.store.load_checkpoint(handoff_id) {
            checkpoint.control_state = HandoffState::Cancelled;
            self.store.save_checkpoint(handoff_id, &checkpoint)?;
        }
        self.store.save_handoff(&handoff)?;
        if let Err(error) = compensate_child_goal_durable(self.store, self.child_goal, handoff_id) {
            let mut handoff = self.store.load_handoff(handoff_id)?;
            handoff.resume_error = Some(format!("{reason}; compensate: {error}"));
            handoff.updated_at_ms = self.runtime.now_ms();
            self.store.save_handoff(&handoff)?;
        }
        self.store.release_lease(handoff_id)?;
        Ok(())
    }
}

pub struct ReturnControlFromShell<'a, S, R> {
    store: &'a S,
    runtime: &'a R,
    child_goal: &'a dyn CollaborativeChildGoalService,
}

impl<'a, S: RecoveryStore, R: HandoffRuntime> ReturnControlFromShell<'a, S, R> {
    pub fn new(
        store: &'a S,
        runtime: &'a R,
        child_goal: &'a dyn CollaborativeChildGoalService,
    ) -> Self {
        Self {
            store,
            runtime,
            child_goal,
        }
    }

    pub fn execute(
        &self,
        handoff_id: &str,
        returned: &HumanShellReturn,
    ) -> Result<(), CollaborativeRecoveryError> {
        let mut handoff = self.store.load_handoff(handoff_id)?;
        let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
        mark_uncertain_tools_on_disconnect(&mut checkpoint);
        handoff.state = transition(handoff.state, HandoffEvent::HumanReturned)?;
        handoff.return_reason = Some("control_returned".into());
        handoff.human_shell_exit_code = returned.exit_code;
        handoff.final_shell_cwd = Some(returned.final_cwd.display().to_string());
        handoff.updated_at_ms = self.runtime.now_ms();
        checkpoint.environment_metadata =
            merge_shell_replay_metadata(&checkpoint.environment_metadata, returned);
        checkpoint.control_state = HandoffState::Returned;
        self.store.save_checkpoint(handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        self.store.release_lease(handoff_id)?;
        if let Err(error) = close_child_goal_durable(
            self.store,
            self.child_goal,
            handoff_id,
            ChildGoalCloseReason::ControlReturned,
        ) {
            let mut handoff = self.store.load_handoff(handoff_id)?;
            handoff.resume_error = Some(format!("child_goal_close: {error}"));
            handoff.updated_at_ms = self.runtime.now_ms();
            self.store.save_handoff(&handoff)?;
        }
        Ok(())
    }
}

pub struct ResumeOrphanedHandoff<'a, S, L, R> {
    store: &'a S,
    launcher: &'a L,
    runtime: &'a R,
    child_goal: &'a dyn CollaborativeChildGoalService,
}

impl<'a, S: RecoveryStore, L: HumanShellLauncher, R: HandoffRuntime>
    ResumeOrphanedHandoff<'a, S, L, R>
{
    pub fn new(
        store: &'a S,
        launcher: &'a L,
        runtime: &'a R,
        child_goal: &'a dyn CollaborativeChildGoalService,
    ) -> Self {
        Self {
            store,
            launcher,
            runtime,
            child_goal,
        }
    }

    pub fn execute(
        &self,
        handoff_id: &str,
        owner: &RecoveryOwner,
    ) -> Result<HumanShellReturn, CollaborativeRecoveryError> {
        let mut handoff = self.store.load_handoff(handoff_id)?;
        if handoff.state != HandoffState::Orphaned {
            return Err(invalid_state(&handoff));
        }
        acquire_recovery_lease(self.store, self.runtime, handoff_id, owner)?;
        let token = self
            .runtime
            .secure_token()
            .map_err(CollaborativeRecoveryError::Token)?;
        let generation = handoff
            .shell_generation
            .checked_add(1)
            .ok_or(HandoffStoreError::InvalidShellGeneration)?;
        self.store.append_shell_session(
            handoff_id,
            &ShellSessionIssueRequest {
                generation,
                token_plaintext: token.clone(),
                now_ms: self.runtime.now_ms(),
            },
        )?;
        handoff.shell_generation = generation;
        handoff.state = transition(handoff.state, HandoffEvent::Resume)?;
        handoff.resume_error = None;
        handoff.updated_at_ms = self.runtime.now_ms();
        let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
        // WAITING の依頼本文は保持しつつ、新 shell の control state は HUMAN_ACTIVE。
        checkpoint.control_state = HandoffState::HumanActive;
        self.store.save_checkpoint(handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::HandoffResumed,
        );
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::HumanShellStarted,
        );

        let request = HumanShellLaunchRequest {
            handoff_id: handoff_id.to_string(),
            token,
            context_version: generation,
            cwd: PathBuf::from(&checkpoint.cwd),
            suggestion_cache_path: suggestion_cache_path(&checkpoint),
        };
        match self.launcher.launch_and_wait(&request) {
            Ok(returned) => {
                ReturnControlFromShell::new(self.store, self.runtime, self.child_goal)
                    .execute(handoff_id, &returned)?;
                Ok(returned)
            }
            Err(error) => {
                let mut current = self.store.load_handoff(handoff_id)?;
                current.state = transition(current.state, HandoffEvent::Orphaned)?;
                current.resume_error = Some(error.to_string());
                current.updated_at_ms = self.runtime.now_ms();
                let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
                checkpoint.control_state = HandoffState::Orphaned;
                self.store.save_checkpoint(handoff_id, &checkpoint)?;
                self.store.save_handoff(&current)?;
                self.store.release_lease(handoff_id)?;
                Err(error.into())
            }
        }
    }
}

fn suggestion_cache_path(checkpoint: &HandoffCheckpoint) -> PathBuf {
    serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
        .ok()
        .and_then(|value| {
            value
                .get("suggestion_cache_path")
                .and_then(|path| path.as_str())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from(&checkpoint.cwd).join(".ai-suggestions.json"))
}

fn merge_shell_replay_metadata(current: &str, returned: &HumanShellReturn) -> String {
    let mut value = serde_json::from_str::<serde_json::Value>(current)
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "shell_session_id".into(),
            returned.shell_session_id.clone().into(),
        );
        object.insert(
            "shell_session_dir".into(),
            returned.shell_session_dir.display().to_string().into(),
        );
        object.insert("shell_log_start".into(), returned.shell_log_start.into());
        object.insert("shell_log_end".into(), returned.shell_log_end.into());
    }
    value.to_string()
}

pub struct ResumeReturnedParent<'a, S, O, R> {
    store: &'a S,
    observer: &'a O,
    runtime: &'a R,
    child_goal: &'a dyn CollaborativeChildGoalService,
}

impl<'a, S: RecoveryStore, O: EnvironmentObserver, R: HandoffRuntime>
    ResumeReturnedParent<'a, S, O, R>
{
    pub fn new(
        store: &'a S,
        observer: &'a O,
        runtime: &'a R,
        child_goal: &'a dyn CollaborativeChildGoalService,
    ) -> Self {
        Self {
            store,
            observer,
            runtime,
            child_goal,
        }
    }

    pub fn prepare(
        &self,
        handoff_id: &str,
        owner: &RecoveryOwner,
    ) -> Result<ParentResumeContext, CollaborativeRecoveryError> {
        reconcile_child_goal_before_parent_resume(self.store, self.child_goal, handoff_id)
            .map_err(|error| CollaborativeRecoveryError::Transition(error.to_string()))?;
        let mut handoff = self.store.load_handoff(handoff_id)?;
        if handoff.state != HandoffState::Returned {
            return Err(invalid_state(&handoff));
        }
        if child_goal_close_blocks_parent_resume(
            &self.store.load_checkpoint(handoff_id)?.child_goal,
        ) {
            let checkpoint = self.store.load_checkpoint(handoff_id)?;
            let message = checkpoint
                .child_goal
                .close_error_message
                .clone()
                .unwrap_or_else(|| "child work close failed".into());
            return Err(CollaborativeRecoveryError::Transition(format!(
                "child_goal_close_blocked: {message}"
            )));
        }
        acquire_recovery_lease(self.store, self.runtime, handoff_id, owner)?;
        handoff.state = transition(handoff.state, HandoffEvent::StartParentResume)?;
        if !handoff
            .resume_error
            .as_ref()
            .is_some_and(|message| message.starts_with("child_goal_close:"))
        {
            handoff.resume_error = None;
        }
        handoff.updated_at_ms = self.runtime.now_ms();
        let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
        mark_uncertain_tools_on_disconnect(&mut checkpoint);
        let uncertain_tool_executions = collect_unknown_tool_ids(&checkpoint);
        let uncertain_tool_details = collect_unknown_tools(&checkpoint);
        let shell_log_start = handoff.shell_log_end.unwrap_or(handoff.shell_log_start);
        let cwd = PathBuf::from(&checkpoint.cwd);
        let observation = self.observer.observe(&cwd, shell_log_start);
        let resume_observation =
            serde_json::to_string(&observation).unwrap_or_else(|_| "{}".into());
        let mut metadata =
            serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
                .unwrap_or_else(|_| serde_json::json!({}));
        if let Some(object) = metadata.as_object_mut() {
            object.insert(
                "resume_observation".into(),
                resume_observation.clone().into(),
            );
        }
        checkpoint.environment_metadata = metadata.to_string();
        checkpoint.control_state = HandoffState::ResumingParent;
        self.store.save_checkpoint(handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::HandoffResumed,
        );
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::ParentResumeStarted,
        );
        Ok(parent_context(
            &handoff,
            &checkpoint,
            uncertain_tool_executions,
            uncertain_tool_details,
            Some(resume_observation),
        ))
    }

    pub fn finish(
        &self,
        handoff_id: &str,
        result: Result<(), String>,
    ) -> Result<(), CollaborativeRecoveryError> {
        let mut handoff = self.store.load_handoff(handoff_id)?;
        let event = if result.is_ok() {
            HandoffEvent::ParentResumeCompleted
        } else {
            HandoffEvent::ParentResumeFailed
        };
        handoff.state = transition(handoff.state, event)?;
        handoff.resume_error = result.err();
        handoff.updated_at_ms = self.runtime.now_ms();
        let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
        checkpoint.control_state = handoff.state;
        self.store.save_checkpoint(handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        record_audit(
            self.store,
            handoff_id,
            if event == HandoffEvent::ParentResumeCompleted {
                CollaborativeAuditKind::ParentResumeCompleted
            } else {
                CollaborativeAuditKind::ParentResumeFailed
            },
        );
        self.store.release_lease(handoff_id)?;
        Ok(())
    }
}

pub struct ReconcileStaleHandoffs<'a, S, R> {
    store: &'a S,
    runtime: &'a R,
}

impl<'a, S: RecoveryStore, R: HandoffRuntime> ReconcileStaleHandoffs<'a, S, R> {
    pub fn new(store: &'a S, runtime: &'a R) -> Self {
        Self { store, runtime }
    }

    /// lease expiry だけでは状態を変えない。owner process 消失を確認した時だけ ORPHANED。
    /// 不完全な CREATING（lease なし・一定時間経過）も初期化失敗として CANCELLED へ。
    pub fn execute<F>(
        &self,
        mut child_goal_for: F,
    ) -> Result<Vec<String>, CollaborativeRecoveryError>
    where
        F: FnMut(&str) -> Box<dyn CollaborativeChildGoalService>,
    {
        const CREATING_STALE_MS: u64 = 120_000;
        let mut reconciled = Vec::new();
        let now = self.runtime.now_ms();
        for handoff in self.store.list_handoffs()? {
            if handoff.state == HandoffState::Creating {
                let lease = self.store.load_lease(&handoff.id)?;
                let stale = now.saturating_sub(handoff.created_at_ms) >= CREATING_STALE_MS;
                let owner_dead = lease
                    .as_ref()
                    .is_some_and(|entry| !self.runtime.process_is_alive(entry.owner_process_id));
                if stale && (lease.is_none() || owner_dead) {
                    let child_goal = child_goal_for(&handoff.id);
                    let reason = if lease.is_none() {
                        "stale_creating_without_lease"
                    } else {
                        "stale_creating_owner_lost"
                    };
                    if self.store.load_checkpoint(&handoff.id).is_err() {
                        reconcile_incomplete_creating_handoff(
                            self.store,
                            child_goal.as_ref(),
                            self.runtime,
                            &handoff.id,
                            reason,
                        )?;
                    } else {
                        CancelHandoff::new(self.store, self.runtime, child_goal.as_ref())
                            .execute(&handoff.id, reason)?;
                    }
                    reconciled.push(handoff.id);
                }
                continue;
            }
            if !matches!(
                handoff.state,
                HandoffState::HumanActive
                    | HandoffState::SideAgentRunning
                    | HandoffState::SideAgentWaitingForHuman
                    | HandoffState::ResumingParent
            ) {
                continue;
            }
            let Some(lease) = self.store.load_lease(&handoff.id)? else {
                continue;
            };
            if self.runtime.process_is_alive(lease.owner_process_id) {
                continue;
            }
            if handoff.state == HandoffState::ResumingParent {
                let mut current = self.store.load_handoff(&handoff.id)?;
                current.state = transition(current.state, HandoffEvent::ParentResumeFailed)?;
                current.resume_error = Some("lease_owner_process_lost".into());
                current.updated_at_ms = self.runtime.now_ms();
                let mut checkpoint = self.store.load_checkpoint(&handoff.id)?;
                checkpoint.control_state = HandoffState::Returned;
                self.store.save_checkpoint(&handoff.id, &checkpoint)?;
                self.store.save_handoff(&current)?;
                self.store.release_lease(&handoff.id)?;
                reconciled.push(handoff.id);
                continue;
            }
            MarkOrphaned::new(self.store, self.runtime)
                .execute(&handoff.id, "lease_owner_process_lost")?;
            reconciled.push(handoff.id);
        }
        Ok(reconciled)
    }
}

pub fn has_unknown_tools(checkpoint: &HandoffCheckpoint) -> bool {
    checkpoint
        .tool_executions
        .iter()
        .any(|tool| tool.status == RecoverableToolStatus::Unknown)
}

fn acquire_recovery_lease<S: LeaseRepository, R: HandoffRuntime>(
    store: &S,
    runtime: &R,
    handoff_id: &str,
    owner: &RecoveryOwner,
) -> Result<(), HandoffStoreError> {
    store.try_acquire_lease(
        handoff_id,
        &LeaseAcquireRequest {
            owner_client_id: owner.client_id.clone(),
            owner_process_id: owner.process_id,
            owner_tty: owner.tty.clone(),
            owner_host: runtime.host_id(),
            owner_uid: runtime.effective_uid(),
            now_ms: runtime.now_ms(),
            lease_timeout_ms: LEASE_TIMEOUT_MS,
        },
    )?;
    Ok(())
}

fn parent_context(
    handoff: &Handoff,
    checkpoint: &HandoffCheckpoint,
    uncertain_tool_executions: Vec<String>,
    uncertain_tool_details: Vec<crate::domain::RecoverableToolExecution>,
    resume_observation: Option<String>,
) -> ParentResumeContext {
    ParentResumeContext {
        handoff_id: handoff.id.clone(),
        parent_task_id: checkpoint.parent_task_id.clone(),
        parent_conversation_id: checkpoint.parent_conversation_id.clone(),
        parent_goal: checkpoint.parent_goal.clone(),
        pending_shell_exec: checkpoint.pending_shell_exec.clone(),
        pending_human_request: handoff.pending_human_request.clone(),
        conversation_summary: checkpoint.conversation_summary.clone(),
        cwd: PathBuf::from(&checkpoint.cwd),
        uncertain_tool_executions,
        uncertain_tool_details,
        resume_observation,
    }
}

fn transition(
    state: HandoffState,
    event: HandoffEvent,
) -> Result<HandoffState, CollaborativeRecoveryError> {
    try_transition(state, event)
        .map_err(|error| CollaborativeRecoveryError::Transition(error.to_string()))
}

fn invalid_state(handoff: &Handoff) -> CollaborativeRecoveryError {
    CollaborativeRecoveryError::InvalidResumeState {
        handoff_id: handoff.id.clone(),
        state: handoff.state,
    }
}
