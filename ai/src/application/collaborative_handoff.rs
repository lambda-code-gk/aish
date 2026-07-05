//! 親 `shell_exec` を human shell handoff へ変換する application service。

use std::path::PathBuf;
use std::sync::Mutex;

use super::{
    child_goal_environment_patch, close_child_goal_durable, compensate_child_goal_durable,
};
use crate::domain::{
    build_candidate_command, collect_unknown_tools, mark_uncertain_tools_on_disconnect,
    try_transition, ChildGoalAchievement, ChildGoalMeta, CollaborativeAgentRole,
    CollaborativeAuditKind, CollaborativePolicy, CommandCandidate, CommandCandidateSource, Handoff,
    HandoffCheckpoint, HandoffEvent, HandoffInitializationFailure, HandoffState,
    RequestedShellExec, SuggestedCommandCache, SuggestedCommandCandidate, SuggestedCommandQueue,
    HANDOFF_SCHEMA_VERSION,
};
use crate::ports::outbound::{
    CheckpointRepository, CollaborativeChildGoalError, CollaborativeChildGoalService,
    CommandCandidateStore, EnvironmentObserver, HandoffAuditRepository, HandoffCandidatePublisher,
    HandoffRepository, HandoffRuntime, HandoffShellSessionStore, HandoffStoreError,
    HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
    LeaseAcquireRequest, LeaseRepository, ParentToolBarrier, ShellSessionIssueRequest,
    SuggestedCommandRecallStore, SuggestedCommandRecallStoreError,
};
use aibe_protocol::{
    HandoffExecutionOutcome, HumanHandoffResult, RequestedCommandCompletion, ShellLogRange,
    UncertainToolExecution,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollaborativeExecutionContext {
    pub role: CollaborativeAgentRole,
    pub policy: CollaborativePolicy,
}

impl CollaborativeExecutionContext {
    pub const fn parent_enabled() -> Self {
        Self {
            role: CollaborativeAgentRole::Parent,
            policy: CollaborativePolicy::Enabled,
        }
    }
    pub const fn disabled() -> Self {
        Self {
            role: CollaborativeAgentRole::Standalone,
            policy: CollaborativePolicy::Disabled,
        }
    }
    pub const fn should_handoff_shell_exec(self) -> bool {
        matches!(self.role, CollaborativeAgentRole::Parent)
            && matches!(self.policy, CollaborativePolicy::Enabled)
    }
}

#[derive(Debug, Clone)]
pub struct ParentShellExecRequest {
    pub parent_task_id: String,
    pub parent_conversation_id: String,
    pub parent_run_id: String,
    pub parent_goal_id: Option<String>,
    pub parent_goal: String,
    pub parent_request_summary: String,
    pub conversation_snapshot: String,
    pub conversation_summary: String,
    pub work_stage_and_plan: String,
    pub memory_space_id: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub tool_call_id: String,
    pub shell_log_start: u64,
    pub suggestion_cache_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum CollaborativeHandoffError {
    #[error("collaborative handoff is not enabled for this execution context")]
    NotApplicable,
    #[error("handoff cwd does not exist: {0}")]
    MissingCwd(String),
    #[error("parent tool barrier failed: {0}")]
    Barrier(String),
    #[error("failed to publish handoff candidate: {0}")]
    Candidate(String),
    #[error(transparent)]
    Store(#[from] HandoffStoreError),
    #[error(transparent)]
    Launch(#[from] HumanShellLaunchError),
    #[error("handoff state transition failed: {0}")]
    Transition(String),
    #[error("failed to generate secure handoff token: {0}")]
    Token(String),
    #[error(transparent)]
    ChildGoal(#[from] CollaborativeChildGoalError),
}

pub trait CollaborativeHandoffStore:
    HandoffRepository
    + CheckpointRepository
    + CommandCandidateStore
    + HandoffShellSessionStore
    + LeaseRepository
    + HandoffAuditRepository
{
}
impl<T> CollaborativeHandoffStore for T where
    T: HandoffRepository
        + CheckpointRepository
        + CommandCandidateStore
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

pub struct CollaborativeShellExecPolicy<'a, S, L, O, B, P, R> {
    context: CollaborativeExecutionContext,
    store: &'a S,
    launcher: &'a L,
    observer: &'a O,
    barrier: &'a B,
    candidate_publisher: &'a P,
    child_goal: &'a dyn CollaborativeChildGoalService,
    runtime: &'a R,
    serial: Mutex<()>,
}

impl<'a, S, L, O, B, P, R> CollaborativeShellExecPolicy<'a, S, L, O, B, P, R>
where
    S: CollaborativeHandoffStore,
    L: HumanShellLauncher,
    O: EnvironmentObserver,
    B: ParentToolBarrier,
    P: HandoffCandidatePublisher,
    R: HandoffRuntime,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: CollaborativeExecutionContext,
        store: &'a S,
        launcher: &'a L,
        observer: &'a O,
        barrier: &'a B,
        candidate_publisher: &'a P,
        child_goal: &'a dyn CollaborativeChildGoalService,
        runtime: &'a R,
    ) -> Self {
        Self {
            context,
            store,
            launcher,
            observer,
            barrier,
            candidate_publisher,
            child_goal,
            runtime,
            serial: Mutex::new(()),
        }
    }

    pub fn intercept(
        &self,
        request: ParentShellExecRequest,
    ) -> Result<HumanHandoffResult, CollaborativeHandoffError> {
        if !self.context.should_handoff_shell_exec() {
            return Err(CollaborativeHandoffError::NotApplicable);
        }
        let _serial = self.serial.lock().expect("collaborative handoff mutex");
        self.barrier
            .wait_for_started_tools()
            .map_err(CollaborativeHandoffError::Barrier)?;
        if !request.cwd.is_dir() {
            return Err(CollaborativeHandoffError::MissingCwd(
                request.cwd.display().to_string(),
            ));
        }

        let now = self.runtime.now_ms();
        let handoff_id = self.runtime.unique_id("handoff");
        let suggestion_cache_path = self.runtime.handoff_suggestion_cache_path(&handoff_id);
        let child_goal_id = self.runtime.unique_id("goal");
        let candidate_text = build_candidate_command(&request.command, &request.args);
        let requested = RequestedShellExec {
            command: request.command.clone(),
            args: request.args.clone(),
            cwd: Some(request.cwd.display().to_string()),
            tool_call_id: Some(request.tool_call_id.clone()),
        };
        let child_goal = ChildGoalMeta {
            id: child_goal_id.clone(),
            handoff_id: handoff_id.clone(),
            parent_goal_id: request.parent_goal_id.clone(),
            work_id: None,
            work_mode: None,
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            close_error_message: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let candidate = CommandCandidate {
            id: self.runtime.unique_id("candidate"),
            command: candidate_text.clone(),
            description: Some("Requested by the parent agent for human review".into()),
            source: CommandCandidateSource::ParentAgent,
            source_run_id: Some(request.parent_run_id.clone()),
            target_handoff_id: handoff_id.clone(),
            created_at_ms: now,
        };
        let before = self.observer.observe(&request.cwd, request.shell_log_start);
        let before_ref = serde_json::to_string(&before).unwrap_or_else(|_| "{}".into());
        let mut handoff = Handoff {
            id: handoff_id.clone(),
            schema_version: HANDOFF_SCHEMA_VERSION,
            parent_task_id: request.parent_task_id.clone(),
            parent_conversation_id: request.parent_conversation_id.clone(),
            parent_run_id: request.parent_run_id.clone(),
            parent_goal_id: request.parent_goal_id.clone(),
            child_goal_id: child_goal_id.clone(),
            side_conversation_id: None,
            state: HandoffState::Creating,
            initial_cwd: request.cwd.display().to_string(),
            final_shell_cwd: None,
            parent_request_summary: request.parent_request_summary.clone(),
            requested_shell_execs: vec![requested.clone()],
            pending_human_request: Some(format!(
                "次のコマンドを確認し、必要なら実行してください: {candidate_text}"
            )),
            conversation_snapshot_ref: "checkpoint.json#conversation_snapshot".into(),
            conversation_summary: request.conversation_summary.clone(),
            checkpoint_ref: "checkpoint.json".into(),
            before_observation_ref: before_ref.clone(),
            after_observation_ref: None,
            shell_log_start: request.shell_log_start,
            shell_log_end: None,
            shell_generation: 1,
            return_reason: None,
            human_shell_exit_code: None,
            resume_error: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.store.save_handoff(&handoff)?;
        let mut init = HandoffInitializationContext::new(
            self.store,
            self.child_goal,
            self.runtime,
            handoff_id.clone(),
        );
        init.state.handoff_created = true;
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::HandoffCreated,
        );
        if let Err(error) = self.store.append_candidate(&handoff_id, &candidate) {
            let primary = format!("candidate append: {error}");
            init.fail(&primary)?;
            return Err(CollaborativeHandoffError::Store(error));
        }
        init.state.candidate_appended = true;
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::CandidateRegistered,
        );
        if let Err(error) = self
            .candidate_publisher
            .publish(&handoff_id, std::slice::from_ref(&candidate_text))
        {
            let primary = format!("candidate publish: {error}");
            init.fail(&primary)?;
            return Err(CollaborativeHandoffError::Candidate(error));
        }
        init.state.candidate_published = true;
        let environment_metadata = serde_json::json!({
            "observation": before,
            "handoff_host_id": self.runtime.host_id(),
            "handoff_uid": self.runtime.effective_uid(),
            "suggestion_cache_path": suggestion_cache_path,
            "work_stage_and_plan": request.work_stage_and_plan,
            "parent_work_id": request.parent_goal_id,
            "memory_space_id": request.memory_space_id,
        })
        .to_string();
        let human_request =
            format!("次のコマンドを確認し、必要なら実行してください: {candidate_text}");
        let checkpoint = HandoffCheckpoint {
            parent_task_id: request.parent_task_id,
            parent_conversation_id: request.parent_conversation_id,
            parent_run_id: request.parent_run_id,
            pending_shell_exec: requested,
            parent_goal: request.parent_goal,
            child_goal,
            conversation_snapshot: request.conversation_snapshot,
            conversation_summary: request.conversation_summary,
            cwd: request.cwd.display().to_string(),
            environment_metadata,
            handoff_id: handoff_id.clone(),
            side_conversation_id: None,
            command_candidates: vec![candidate],
            shell_log_start: request.shell_log_start,
            control_state: HandoffState::Creating,
            provider_metadata: None,
            tool_executions: vec![],
        };
        // Recovery checkpoint is durable before the PTY process is spawned.
        self.store.save_checkpoint(&handoff_id, &checkpoint)?;
        init.state.checkpoint_created = true;
        let mut child_goal_meta = checkpoint.child_goal.clone();
        match self.child_goal.create_child_goal(
            &mut child_goal_meta,
            &request.cwd,
            &checkpoint.parent_goal,
            &request.parent_request_summary,
            &candidate_text,
            &human_request,
        ) {
            Ok(()) => {
                init.state.child_work_opened = true;
                let mut saved = self.store.load_checkpoint(&handoff_id)?;
                saved.child_goal = child_goal_meta.clone();
                let metadata = child_goal_environment_patch(&saved);
                saved.environment_metadata = metadata.to_string();
                if let Err(error) = self.store.save_checkpoint(&handoff_id, &saved) {
                    let primary = format!("child goal metadata save: {error}");
                    init.fail(&primary)?;
                    return Err(CollaborativeHandoffError::Store(error));
                }
            }
            Err(error) => {
                let create_message = error.to_string();
                init.persist_create_error(&child_goal_meta, &create_message)?;
                if child_goal_meta.work_mode.is_some() {
                    init.state.child_work_opened = true;
                }
                init.fail(&format!("child_goal_create: {create_message}"))?;
                return Err(CollaborativeHandoffError::ChildGoal(error));
            }
        }
        let token = self.runtime.secure_token().map_err(|error| {
            let primary = format!("token: {error}");
            let _ = init.fail(&primary);
            CollaborativeHandoffError::Token(error)
        })?;
        if self
            .store
            .append_shell_session(
                &handoff_id,
                &ShellSessionIssueRequest {
                    generation: 1,
                    token_plaintext: token.clone(),
                    now_ms: now,
                },
            )
            .is_err()
        {
            let message = "failed to persist shell session".to_string();
            init.fail(&message)?;
            return Err(CollaborativeHandoffError::Store(HandoffStoreError::Write(
                message,
            )));
        }
        init.state.shell_session_issued = true;
        if self
            .store
            .try_acquire_lease(
                &handoff_id,
                &LeaseAcquireRequest {
                    owner_client_id: format!("ai-parent-{}", self.runtime.process_id()),
                    owner_process_id: self.runtime.process_id(),
                    owner_tty: self.runtime.tty(),
                    owner_host: self.runtime.host_id(),
                    owner_uid: self.runtime.effective_uid(),
                    now_ms: now,
                    lease_timeout_ms: 120_000,
                },
            )
            .is_err()
        {
            let message = "failed to acquire handoff lease".to_string();
            init.fail(&message)?;
            return Err(CollaborativeHandoffError::Store(HandoffStoreError::Write(
                message,
            )));
        }
        init.state.lease_acquired = true;
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::LeaseAcquired,
        );
        handoff = self.store.load_handoff(&handoff_id)?;
        handoff.state = transition(handoff.state, HandoffEvent::ShellReady)?;
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;
        init.state.shell_ready = true;
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::HumanShellStarted,
        );

        let launch_result = self.launcher.launch_and_wait(&HumanShellLaunchRequest {
            handoff_id: handoff_id.clone(),
            token,
            context_version: handoff.shell_generation,
            cwd: request.cwd,
            suggestion_cache_path,
        });
        let returned = match launch_result {
            Ok(returned) => returned,
            Err(error) => {
                // spawn 前の失敗は durable checkpoint を CANCELLED にする。spawn 後に
                // normal-return marker が無い場合は成果を推測せず ORPHANED。
                handoff = self.store.load_handoff(&handoff_id)?;
                let event = match error {
                    HumanShellLaunchError::MissingReturnMarker => HandoffEvent::Orphaned,
                    HumanShellLaunchError::MissingCwd(_) | HumanShellLaunchError::Failed(_) => {
                        HandoffEvent::ShellLaunchFailed
                    }
                };
                handoff.state = transition(handoff.state, event)?;
                handoff.return_reason = Some(
                    match event {
                        HandoffEvent::Orphaned => "abnormal_shell_exit",
                        _ => "shell_launch_failed",
                    }
                    .into(),
                );
                handoff.updated_at_ms = self.runtime.now_ms();
                let mut checkpoint = self.store.load_checkpoint(&handoff_id)?;
                checkpoint.control_state = handoff.state;
                self.store.save_checkpoint(&handoff_id, &checkpoint)?;
                self.store.save_handoff(&handoff)?;
                if matches!(event, HandoffEvent::Orphaned) {
                    record_audit(
                        self.store,
                        &handoff_id,
                        CollaborativeAuditKind::HumanShellOrphaned,
                    );
                }
                if matches!(event, HandoffEvent::ShellLaunchFailed) {
                    let _ = init.fail("shell_launch_failed");
                }
                self.store.release_lease(&handoff_id)?;
                record_audit(self.store, &handoff_id, CollaborativeAuditKind::LeaseLost);
                return Err(error.into());
            }
        };
        // side agent が shell lifetime 中に状態を更新し得るため、保存済み状態を再読込する。
        handoff = self.store.load_handoff(&handoff_id)?;
        handoff.state = transition(handoff.state, HandoffEvent::HumanReturned)?;
        handoff.return_reason = Some("control_returned".into());
        handoff.human_shell_exit_code = returned.exit_code;
        handoff.final_shell_cwd = Some(returned.final_cwd.display().to_string());
        let after = self
            .observer
            .observe(&returned.final_cwd, returned.shell_log_start);
        let after_ref = serde_json::to_string(&after).unwrap_or_else(|_| "{}".into());
        handoff.after_observation_ref = Some(after_ref.clone());
        handoff.shell_log_end = Some(returned.shell_log_end);
        handoff.updated_at_ms = self.runtime.now_ms();
        let mut checkpoint = self.store.load_checkpoint(&handoff_id)?;
        checkpoint.environment_metadata =
            merge_shell_replay_metadata(&checkpoint.environment_metadata, &returned);
        mark_uncertain_tools_on_disconnect(&mut checkpoint);
        let uncertain_tool_executions = uncertain_tools_for_result(&checkpoint);
        checkpoint.control_state = HandoffState::Returned;
        self.store.save_checkpoint(&handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::HumanShellReturned,
        );
        self.store.release_lease(&handoff_id)?;
        record_audit(self.store, &handoff_id, CollaborativeAuditKind::LeaseLost);
        if let Err(error) = close_child_goal_durable(
            self.store,
            self.child_goal,
            &handoff_id,
            crate::domain::ChildGoalCloseReason::ControlReturned,
        ) {
            handoff = self.store.load_handoff(&handoff_id)?;
            handoff.resume_error = Some(format!("child_goal_close: {error}"));
            handoff.updated_at_ms = self.runtime.now_ms();
            self.store.save_handoff(&handoff)?;
        }
        Ok(HumanHandoffResult {
            handoff_id,
            execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
            return_reason: Some("control_returned".into()),
            human_shell_exit_code: returned.exit_code,
            requested_command: Some(candidate_text),
            requested_command_completion: RequestedCommandCompletion::Unknown,
            final_shell_cwd: handoff.final_shell_cwd,
            shell_log_range: Some(ShellLogRange {
                start: returned.shell_log_start,
                end: Some(returned.shell_log_end),
            }),
            child_goal_summary: Some(
                "Human control returned; child-goal achievement remains unknown.".into(),
            ),
            side_conversation_summary: None,
            before_observation_ref: Some(before_ref),
            after_observation_ref: Some(after_ref),
            uncertain_tool_executions,
        })
    }
}

#[derive(Default)]
struct HandoffInitializationState {
    handoff_created: bool,
    candidate_appended: bool,
    candidate_published: bool,
    checkpoint_created: bool,
    child_work_opened: bool,
    shell_session_issued: bool,
    lease_acquired: bool,
    shell_ready: bool,
}

struct CompensationOutcome {
    attempted: bool,
    succeeded: bool,
    errors: Vec<String>,
    remaining_resources: Vec<String>,
}

struct HandoffInitializationContext<'a, S, R> {
    store: &'a S,
    child_goal: &'a dyn CollaborativeChildGoalService,
    runtime: &'a R,
    handoff_id: String,
    state: HandoffInitializationState,
}

impl<'a, S, R> HandoffInitializationContext<'a, S, R>
where
    S: CollaborativeHandoffStore,
    R: HandoffRuntime,
{
    fn new(
        store: &'a S,
        child_goal: &'a dyn CollaborativeChildGoalService,
        runtime: &'a R,
        handoff_id: String,
    ) -> Self {
        Self {
            store,
            child_goal,
            runtime,
            handoff_id,
            state: HandoffInitializationState::default(),
        }
    }

    fn fail(&self, primary_error: &str) -> Result<(), CollaborativeHandoffError> {
        let outcome = self.compensate(primary_error);
        let failure = HandoffInitializationFailure {
            primary_error: primary_error.to_string(),
            compensation_attempted: outcome.attempted,
            compensation_succeeded: outcome.succeeded,
            compensation_errors: outcome.errors.clone(),
            remaining_resources: outcome.remaining_resources.clone(),
            manual_recovery_required: !outcome.succeeded || !outcome.remaining_resources.is_empty(),
            occurred_at_ms: self.runtime.now_ms(),
        };
        self.persist_initialization_failure(&failure)?;
        if !outcome.succeeded {
            return Err(CollaborativeHandoffError::Transition(
                failure.combined_error_message(),
            ));
        }
        Ok(())
    }

    fn compensate(&self, _primary_error: &str) -> CompensationOutcome {
        let mut errors = Vec::new();
        let mut remaining = Vec::new();
        let mut succeeded = true;
        let attempted = true;

        if self.state.child_work_opened {
            if let Err(error) =
                compensate_child_goal_durable(self.store, self.child_goal, &self.handoff_id)
            {
                succeeded = false;
                errors.push(error.to_string());
                remaining.push("child_work".into());
            }
        }
        if self.state.lease_acquired {
            if let Err(error) = self.store.release_lease(&self.handoff_id) {
                succeeded = false;
                errors.push(error.to_string());
                remaining.push("lease".into());
            }
        }
        if self.state.shell_session_issued {
            if let Err(error) = self.invalidate_shell_sessions() {
                succeeded = false;
                errors.push(error);
                remaining.push("shell_session".into());
            }
        }
        if self.state.candidate_published {
            if let Err(error) = clear_handoff_suggestion_cache(self.runtime, &self.handoff_id) {
                succeeded = false;
                errors.push(error);
                remaining.push("candidate_cache".into());
            }
        }
        if let Err(error) = self.cancel_handoff(_primary_error) {
            succeeded = false;
            errors.push(error.to_string());
            remaining.push("handoff_state".into());
        }
        CompensationOutcome {
            attempted,
            succeeded,
            errors,
            remaining_resources: remaining,
        }
    }

    fn invalidate_shell_sessions(&self) -> Result<(), String> {
        if !self.state.checkpoint_created {
            return Ok(());
        }
        let mut checkpoint = self
            .store
            .load_checkpoint(&self.handoff_id)
            .map_err(|e| e.to_string())?;
        let mut metadata =
            serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
                .unwrap_or_else(|_| serde_json::json!({}));
        if let Some(object) = metadata.as_object_mut() {
            object.insert("shell_sessions_invalidated".into(), true.into());
        }
        checkpoint.environment_metadata = metadata.to_string();
        self.store
            .save_checkpoint(&self.handoff_id, &checkpoint)
            .map_err(|e| e.to_string())
    }

    fn persist_initialization_failure(
        &self,
        failure: &HandoffInitializationFailure,
    ) -> Result<(), CollaborativeHandoffError> {
        let message = failure.combined_error_message();
        if self.state.checkpoint_created {
            let mut checkpoint = self.store.load_checkpoint(&self.handoff_id)?;
            let mut metadata =
                serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
                    .unwrap_or_else(|_| serde_json::json!({}));
            if let Some(object) = metadata.as_object_mut() {
                object.insert(
                    "initialization_failure".into(),
                    serde_json::to_value(failure).unwrap_or_else(|_| serde_json::json!({})),
                );
                object.insert("initialization_error".into(), message.clone().into());
            }
            checkpoint.environment_metadata = metadata.to_string();
            if self.state.child_work_opened
                && (checkpoint.child_goal.close_state.is_none()
                    || checkpoint.child_goal.close_state
                        == Some(crate::domain::ChildGoalCloseState::Open))
            {
                checkpoint.child_goal.close_state =
                    Some(crate::domain::ChildGoalCloseState::Failed);
                checkpoint.child_goal.close_error_message = Some(message.clone());
            }
            self.store.save_checkpoint(&self.handoff_id, &checkpoint)?;
        }
        let mut handoff = self.store.load_handoff(&self.handoff_id)?;
        handoff.resume_error = Some(format!("initialization: {message}"));
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;
        Ok(())
    }

    fn persist_create_error(
        &self,
        child_goal_meta: &ChildGoalMeta,
        message: &str,
    ) -> Result<(), CollaborativeHandoffError> {
        if !self.state.checkpoint_created {
            return Ok(());
        }
        let mut checkpoint = self.store.load_checkpoint(&self.handoff_id)?;
        checkpoint.child_goal = child_goal_meta.clone();
        let mut metadata =
            serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
                .unwrap_or_else(|_| serde_json::json!({}));
        if let Some(object) = metadata.as_object_mut() {
            object.insert("child_goal_create_error".into(), message.into());
            object.insert("initialization_error".into(), message.into());
        }
        checkpoint.environment_metadata = metadata.to_string();
        self.store.save_checkpoint(&self.handoff_id, &checkpoint)?;
        let mut handoff = self.store.load_handoff(&self.handoff_id)?;
        handoff.resume_error = Some(format!("initialization: {message}"));
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;
        Ok(())
    }

    fn cancel_handoff(&self, message: &str) -> Result<(), HandoffStoreError> {
        let mut handoff = self.store.load_handoff(&self.handoff_id)?;
        if handoff.state == HandoffState::Creating {
            handoff.state = try_transition(handoff.state, HandoffEvent::Cancel)
                .map_err(|e| HandoffStoreError::Write(e.to_string()))?;
        } else if handoff.state == HandoffState::HumanActive && !self.state.shell_ready {
            handoff.state = try_transition(handoff.state, HandoffEvent::ShellLaunchFailed)
                .map_err(|e| HandoffStoreError::Write(e.to_string()))?;
        }
        handoff.return_reason = Some("initialization_failed".into());
        handoff.resume_error = Some(message.to_string());
        handoff.updated_at_ms = self.runtime.now_ms();
        if self.state.checkpoint_created {
            let mut checkpoint = self.store.load_checkpoint(&self.handoff_id)?;
            checkpoint.control_state = handoff.state;
            self.store.save_checkpoint(&self.handoff_id, &checkpoint)?;
        }
        self.store.save_handoff(&handoff)
    }
}

fn uncertain_tools_for_result(checkpoint: &HandoffCheckpoint) -> Vec<UncertainToolExecution> {
    collect_unknown_tools(checkpoint)
        .into_iter()
        .map(|tool| UncertainToolExecution {
            tool_call_id: tool.tool_call_id,
            tool_name: tool.tool_name,
            status: "unknown".into(),
        })
        .collect()
}

fn clear_handoff_suggestion_cache<R: HandoffRuntime>(
    runtime: &R,
    handoff_id: &str,
) -> Result<(), String> {
    let path = runtime.handoff_suggestion_cache_path(handoff_id);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn reconcile_incomplete_creating_handoff<S, C, R>(
    store: &S,
    child_goal: &C,
    runtime: &R,
    handoff_id: &str,
    reason: &str,
) -> Result<(), HandoffStoreError>
where
    S: HandoffRepository + CheckpointRepository + LeaseRepository,
    C: CollaborativeChildGoalService + ?Sized,
    R: HandoffRuntime,
{
    let mut handoff = store.load_handoff(handoff_id)?;
    if handoff.state != HandoffState::Creating {
        return Ok(());
    }
    handoff.state = try_transition(handoff.state, HandoffEvent::Cancel)
        .map_err(|e| HandoffStoreError::Write(e.to_string()))?;
    handoff.return_reason = Some(reason.into());
    handoff.resume_error = Some(format!("initialization: {reason}"));
    handoff.updated_at_ms = runtime.now_ms();
    store.save_handoff(&handoff)?;
    let _ = store.release_lease(handoff_id);
    let _ = clear_handoff_suggestion_cache(runtime, handoff_id);
    if let Ok(checkpoint) = store.load_checkpoint(handoff_id) {
        if checkpoint.child_goal.work_mode.is_some() {
            let _ = compensate_child_goal_durable(store, child_goal, handoff_id);
        }
        let mut updated = store.load_checkpoint(handoff_id)?;
        updated.control_state = HandoffState::Cancelled;
        store.save_checkpoint(handoff_id, &updated)?;
    }
    Ok(())
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

pub fn suggestion_cache_path_from_checkpoint(checkpoint: &HandoffCheckpoint) -> Option<PathBuf> {
    serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
        .ok()?
        .get("suggestion_cache_path")?
        .as_str()
        .map(PathBuf::from)
}

pub fn persist_handoff_candidates_for_recall<S: SuggestedCommandRecallStore>(
    store: &S,
    ai_session_id: &str,
    handoff_id: &str,
    commands: &[String],
    captured_at: &str,
    shell: &str,
) -> Result<(), SuggestedCommandRecallStoreError> {
    if commands.is_empty() {
        return Ok(());
    }
    let mut cache = store
        .load()?
        .unwrap_or_else(|| SuggestedCommandCache::new(ai_session_id, shell, captured_at));
    cache.updated_at = captured_at.to_string();
    cache.append_queue(SuggestedCommandQueue {
        turn_id: format!("handoff:{handoff_id}"),
        captured_at: captured_at.to_string(),
        candidates: commands
            .iter()
            .map(|command| SuggestedCommandCandidate {
                text: command.clone(),
                language: "shell".into(),
                bytes: command.len(),
            })
            .collect(),
    });
    store.save(&cache)
}

fn transition(
    state: HandoffState,
    event: HandoffEvent,
) -> Result<HandoffState, CollaborativeHandoffError> {
    try_transition(state, event).map_err(|e| CollaborativeHandoffError::Transition(e.to_string()))
}
