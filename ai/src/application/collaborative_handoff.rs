//! 親 `shell_exec` を human shell handoff へ変換する application service。

use std::path::PathBuf;
use std::sync::Mutex;

use crate::domain::{
    build_candidate_command, try_transition, ChildGoalAchievement, ChildGoalMeta,
    CollaborativeAgentRole, CollaborativePolicy, CommandCandidate, CommandCandidateSource, Handoff,
    HandoffCheckpoint, HandoffEvent, HandoffState, RequestedShellExec, SuggestedCommandCache,
    SuggestedCommandCandidate, SuggestedCommandQueue, HANDOFF_SCHEMA_VERSION,
};
use crate::ports::outbound::{
    CheckpointRepository, CommandCandidateStore, EnvironmentObserver, HandoffCandidatePublisher,
    HandoffRepository, HandoffRuntime, HandoffShellSessionStore, HandoffStoreError,
    HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher, ParentToolBarrier,
    ShellSessionIssueRequest, SuggestedCommandRecallStore, SuggestedCommandRecallStoreError,
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
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub tool_call_id: String,
    pub shell_log_start: u64,
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
}

pub trait CollaborativeHandoffStore:
    HandoffRepository + CheckpointRepository + CommandCandidateStore + HandoffShellSessionStore
{
}
impl<T> CollaborativeHandoffStore for T where
    T: HandoffRepository + CheckpointRepository + CommandCandidateStore + HandoffShellSessionStore
{
}

pub struct CollaborativeShellExecPolicy<'a, S, L, O, B, P, R> {
    context: CollaborativeExecutionContext,
    store: &'a S,
    launcher: &'a L,
    observer: &'a O,
    barrier: &'a B,
    candidate_publisher: &'a P,
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
    pub fn new(
        context: CollaborativeExecutionContext,
        store: &'a S,
        launcher: &'a L,
        observer: &'a O,
        barrier: &'a B,
        candidate_publisher: &'a P,
        runtime: &'a R,
    ) -> Self {
        Self {
            context,
            store,
            launcher,
            observer,
            barrier,
            candidate_publisher,
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
            close_reason: None,
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
                "Review and, if appropriate, run: {candidate_text}"
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
        self.store.append_candidate(&handoff_id, &candidate)?;
        self.candidate_publisher
            .publish(&handoff_id, std::slice::from_ref(&candidate_text))
            .map_err(CollaborativeHandoffError::Candidate)?;
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
            environment_metadata: before_ref.clone(),
            handoff_id: handoff_id.clone(),
            side_conversation_id: None,
            command_candidates: vec![candidate],
            shell_log_start: request.shell_log_start,
            control_state: HandoffState::Creating,
            provider_metadata: None,
        };
        // Recovery checkpoint is durable before the PTY process is spawned.
        self.store.save_checkpoint(&handoff_id, &checkpoint)?;
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
        handoff.state = transition(handoff.state, HandoffEvent::ShellReady)?;
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;

        let returned = self.launcher.launch_and_wait(&HumanShellLaunchRequest {
            handoff_id: handoff_id.clone(),
            token,
            context_version: HANDOFF_SCHEMA_VERSION,
            cwd: request.cwd,
        })?;
        handoff.state = transition(handoff.state, HandoffEvent::HumanReturned)?;
        handoff.return_reason = Some("control_returned".into());
        handoff.human_shell_exit_code = returned.exit_code;
        handoff.final_shell_cwd = Some(returned.final_cwd.display().to_string());
        let after = self
            .observer
            .observe(&returned.final_cwd, request.shell_log_start);
        let after_ref = serde_json::to_string(&after).unwrap_or_else(|_| "{}".into());
        handoff.after_observation_ref = Some(after_ref.clone());
        handoff.shell_log_end = after.shell_log_end;
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;
        handoff.state = transition(handoff.state, HandoffEvent::StartParentResume)?;
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;
        handoff.state = transition(handoff.state, HandoffEvent::ParentResumeCompleted)?;
        handoff.updated_at_ms = self.runtime.now_ms();
        self.store.save_handoff(&handoff)?;

        Ok(HumanHandoffResult {
            handoff_id,
            execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
            return_reason: Some("control_returned".into()),
            human_shell_exit_code: returned.exit_code,
            requested_command: Some(candidate_text),
            requested_command_completion: RequestedCommandCompletion::Unknown,
            final_shell_cwd: handoff.final_shell_cwd,
            shell_log_range: Some(ShellLogRange {
                start: request.shell_log_start,
                end: handoff.shell_log_end,
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
