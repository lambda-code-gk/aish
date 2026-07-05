//! 親 `shell_exec` を human shell handoff へ変換する application service。

use std::path::PathBuf;
use std::sync::Mutex;

use super::{
    child_goal_environment_patch, close_child_goal_durable, compensate_child_goal_durable,
};
use crate::domain::{
    build_candidate_command, mark_running_tools_unknown, try_transition, ChildGoalAchievement,
    ChildGoalMeta, CollaborativeAgentRole, CollaborativeAuditKind, CollaborativePolicy,
    CommandCandidate, CommandCandidateSource, Handoff, HandoffCheckpoint, HandoffEvent,
    HandoffState, RequestedShellExec, SuggestedCommandCache, SuggestedCommandCandidate,
    SuggestedCommandQueue, HANDOFF_SCHEMA_VERSION,
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
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
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
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::HandoffCreated,
        );
        self.store.append_candidate(&handoff_id, &candidate)?;
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::CandidateRegistered,
        );
        self.candidate_publisher
            .publish(&handoff_id, std::slice::from_ref(&candidate_text))
            .map_err(CollaborativeHandoffError::Candidate)?;
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
                let mut saved = self.store.load_checkpoint(&handoff_id)?;
                saved.child_goal = child_goal_meta.clone();
                let metadata = child_goal_environment_patch(&saved);
                saved.environment_metadata = metadata.to_string();
                self.store.save_checkpoint(&handoff_id, &saved)?;
            }
            Err(error) => {
                let mut checkpoint = self.store.load_checkpoint(&handoff_id)?;
                checkpoint.child_goal = child_goal_meta.clone();
                let mut metadata =
                    serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
                        .unwrap_or_else(|_| serde_json::json!({}));
                if let Some(object) = metadata.as_object_mut() {
                    object.insert("child_goal_create_error".into(), error.to_string().into());
                    if let Some(root_id) = child_goal_meta.auto_root_work_id {
                        object.insert("auto_root_work_id".into(), root_id.into());
                    }
                }
                checkpoint.environment_metadata = metadata.to_string();
                self.store.save_checkpoint(&handoff_id, &checkpoint)?;
            }
        }
        let token = self
            .runtime
            .secure_token()
            .map_err(CollaborativeHandoffError::Token)?;
        self.store.append_shell_session(
            &handoff_id,
            &ShellSessionIssueRequest {
                generation: 1,
                token_plaintext: token.clone(),
                now_ms: now,
            },
        )?;
        self.store.try_acquire_lease(
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
        )?;
        record_audit(
            self.store,
            &handoff_id,
            CollaborativeAuditKind::LeaseAcquired,
        );
        handoff.state = transition(handoff.state, HandoffEvent::ShellReady)?;
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;
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
                    let _ = compensate_child_goal_durable(self.store, self.child_goal, &handoff_id);
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
        mark_running_tools_unknown(&mut checkpoint);
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
            uncertain_tool_executions: Vec::new(),
        })
    }
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
