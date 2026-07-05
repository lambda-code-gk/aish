//! Human shell 内 `ai` を handoff の side agent へ接続する application service。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::domain::{
    deterministic_candidate_id, try_transition, CollaborativeAuditKind, CollaborativeHandoffReport,
    CommandCandidate, CommandCandidateSource, Handoff, HandoffEvent, HandoffState,
    RequestHumanAction,
};
use crate::ports::outbound::{
    CheckpointRepository, CommandCandidateStore, EnvironmentObserver, HandoffAuditRepository,
    HandoffRepository, HandoffRuntime, HandoffShellSessionStore, HandoffStoreError,
    LeaseAcquireRequest, LeaseRepository, SideRunLockRepository,
};

pub const HANDOFF_ENV_KEYS: [&str; 4] = [
    "AISH_CONTROL_MODE",
    "AISH_HANDOFF_ID",
    "AISH_HANDOFF_TOKEN",
    "AISH_HANDOFF_CONTEXT_VERSION",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollaborativeShellEnvironment {
    pub handoff_id: String,
    pub token: String,
    pub generation: u32,
}

impl CollaborativeShellEnvironment {
    pub fn from_map(env: &HashMap<String, String>) -> Result<Option<Self>, SideAgentError> {
        let present = HANDOFF_ENV_KEYS
            .iter()
            .filter(|key| env.contains_key(**key))
            .count();
        if present == 0 {
            return Ok(None);
        }
        if present != HANDOFF_ENV_KEYS.len() {
            return Err(SideAgentError::IncompleteEnvironment);
        }
        if env.get("AISH_CONTROL_MODE").map(String::as_str) != Some("human-shell") {
            return Err(SideAgentError::InvalidControlMode);
        }
        let generation = env["AISH_HANDOFF_CONTEXT_VERSION"]
            .parse::<u32>()
            .map_err(|_| SideAgentError::InvalidGeneration)?;
        Ok(Some(Self {
            handoff_id: env["AISH_HANDOFF_ID"].clone(),
            token: env["AISH_HANDOFF_TOKEN"].clone(),
            generation,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideAgentInvocation {
    pub standalone: bool,
    pub collaborative_requested: bool,
    pub bare: bool,
    pub user_note: Option<String>,
    pub client_id: String,
    pub process_id: u32,
    pub tty: Option<String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HumanControlReturned {
    pub pending_request: String,
    pub shell_log_delta: String,
    pub current_cwd: String,
    pub current_observation: String,
    pub user_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SideTurn {
    pub handoff_id: String,
    pub conversation_id: String,
    pub system_instruction: String,
    pub control_returned: Option<HumanControlReturned>,
    pub collaborative_handoff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideAgentDispatch {
    Standalone,
    Normal,
    PromptForInput { handoff_id: String },
    Run(SideTurn),
}

#[derive(Debug, thiserror::Error)]
pub enum SideAgentError {
    #[error("incomplete collaborative handoff environment; run `ai --standalone` to ignore it")]
    IncompleteEnvironment,
    #[error("invalid collaborative control mode")]
    InvalidControlMode,
    #[error("invalid handoff generation")]
    InvalidGeneration,
    #[error("invalid or stale handoff token")]
    InvalidToken,
    #[error("handoff host ID does not match this host")]
    HostMismatch,
    #[error("handoff UID does not match the effective UID")]
    UidMismatch,
    #[error("handoff identity metadata is missing or invalid")]
    InvalidIdentityMetadata,
    #[error("nested `ai --collaborative` is not allowed inside a human shell")]
    NestedCollaborative,
    #[error("side agent is already running; use `ai status`")]
    AlreadyRunning,
    #[error("handoff is orphaned; run `ai resume`")]
    Orphaned,
    #[error("handoff is no longer active")]
    Inactive,
    #[error("handoff state transition failed: {0}")]
    Transition(String),
    #[error(transparent)]
    Store(#[from] HandoffStoreError),
}

pub trait SideAgentStore:
    HandoffRepository
    + CheckpointRepository
    + HandoffShellSessionStore
    + LeaseRepository
    + SideRunLockRepository
    + CommandCandidateStore
    + HandoffAuditRepository
{
}

impl<T> SideAgentStore for T where
    T: HandoffRepository
        + CheckpointRepository
        + HandoffShellSessionStore
        + LeaseRepository
        + SideRunLockRepository
        + CommandCandidateStore
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

#[derive(Debug, Clone, Default)]
pub struct SideAgentDispatchOptions<'a> {
    pub memory_prompt_block: Option<&'a str>,
}

pub struct StartOrResumeSideAgent<'a, S, O, R> {
    store: &'a S,
    observer: &'a O,
    runtime: &'a R,
}

impl<'a, S, O, R> StartOrResumeSideAgent<'a, S, O, R>
where
    S: SideAgentStore,
    O: EnvironmentObserver,
    R: HandoffRuntime,
{
    pub fn new(store: &'a S, observer: &'a O, runtime: &'a R) -> Self {
        Self {
            store,
            observer,
            runtime,
        }
    }

    /// token / identity / state を検証する。Memory query より先に呼ぶ。
    pub fn validate_shell_participation(
        &self,
        env: &CollaborativeShellEnvironment,
        invocation: &SideAgentInvocation,
    ) -> Result<Option<SideAgentDispatch>, SideAgentError> {
        let mut handoff = self.store.load_handoff(&env.handoff_id)?;
        let sessions = self.store.list_shell_sessions(&env.handoff_id)?;
        if !crate::domain::validate_shell_token(&sessions, &env.token, env.generation)
            || env.generation != handoff.shell_generation
        {
            record_audit(
                self.store,
                &env.handoff_id,
                CollaborativeAuditKind::StaleTokenRejected,
            );
            return Err(SideAgentError::InvalidToken);
        }
        let mut checkpoint = self.store.load_checkpoint(&env.handoff_id)?;
        validate_identity(
            &checkpoint.environment_metadata,
            &self.runtime.host_id(),
            self.runtime.effective_uid(),
        )?;
        if invocation.collaborative_requested {
            return Err(SideAgentError::NestedCollaborative);
        }

        match handoff.state {
            HandoffState::HumanActive if invocation.bare => {
                return Ok(Some(SideAgentDispatch::PromptForInput {
                    handoff_id: handoff.id,
                }));
            }
            HandoffState::SideAgentRunning => {
                if !try_recover_stale_side_agent(
                    self.store,
                    self.runtime,
                    &env.handoff_id,
                    &mut handoff,
                    &mut checkpoint,
                )? {
                    return Err(SideAgentError::AlreadyRunning);
                }
            }
            HandoffState::Orphaned => return Err(SideAgentError::Orphaned),
            HandoffState::Returned | HandoffState::Completed | HandoffState::Cancelled => {
                return Err(SideAgentError::Inactive);
            }
            HandoffState::Creating | HandoffState::ResumingParent => {
                return Err(SideAgentError::Inactive);
            }
            HandoffState::HumanActive | HandoffState::SideAgentWaitingForHuman => {}
        }

        if matches!(
            handoff.state,
            HandoffState::HumanActive | HandoffState::SideAgentWaitingForHuman
        ) {
            let lifetime_lease = self.store.load_lease(&env.handoff_id)?;
            if lifetime_lease
                .as_ref()
                .is_none_or(|lease| lease.lease_expires_at_ms <= self.runtime.now_ms())
            {
                return Err(SideAgentError::Inactive);
            }
        }
        Ok(None)
    }

    /// spec §13.5 の判定順（standalone → env → token/identity → state）を保つ。
    pub fn dispatch(
        &self,
        env: Option<CollaborativeShellEnvironment>,
        invocation: &SideAgentInvocation,
        options: SideAgentDispatchOptions<'_>,
    ) -> Result<SideAgentDispatch, SideAgentError> {
        if invocation.standalone {
            return Ok(SideAgentDispatch::Standalone);
        }
        let Some(env) = env else {
            return Ok(SideAgentDispatch::Normal);
        };
        if let Some(early) = self.validate_shell_participation(&env, invocation)? {
            return Ok(early);
        }

        let lease_request = LeaseAcquireRequest {
            owner_client_id: invocation.client_id.clone(),
            owner_process_id: invocation.process_id,
            owner_tty: invocation.tty.clone(),
            owner_host: self.runtime.host_id(),
            owner_uid: self.runtime.effective_uid(),
            now_ms: self.runtime.now_ms(),
            lease_timeout_ms: 120_000,
        };
        let mut created_conversation = false;
        let mut was_waiting = false;
        let mut conversation_id = String::new();
        self.store
            .start_side_run_atomically(
                &env.handoff_id,
                &lease_request,
                &|pid| self.runtime.process_is_alive(pid),
                &mut |handoff, checkpoint| {
                    match handoff.state {
                        HandoffState::HumanActive | HandoffState::SideAgentWaitingForHuman => {}
                        HandoffState::SideAgentRunning => {
                            return Err(HandoffStoreError::LeaseConflict);
                        }
                        _ => {
                            return Err(HandoffStoreError::Write(
                                "handoff inactive for side run".into(),
                            ));
                        }
                    }
                    was_waiting = handoff.state == HandoffState::SideAgentWaitingForHuman;
                    conversation_id = match handoff.side_conversation_id.clone() {
                        Some(id) => id,
                        None => {
                            created_conversation = true;
                            let id = self.runtime.unique_id("side-conversation");
                            handoff.side_conversation_id = Some(id.clone());
                            checkpoint.side_conversation_id = Some(id.clone());
                            id
                        }
                    };
                    handoff.state = transition(
                        handoff.state,
                        if was_waiting {
                            HandoffEvent::SideAgentResumed
                        } else {
                            HandoffEvent::StartSideAgent
                        },
                    )
                    .map_err(|error| HandoffStoreError::Write(error.to_string()))?;
                    handoff.conversation_summary = update_summary(
                        &handoff.conversation_summary,
                        if was_waiting {
                            "human control returned; side run resumed"
                        } else {
                            "side run started"
                        },
                    );
                    checkpoint.control_state = handoff.state;
                    checkpoint.conversation_summary = handoff.conversation_summary.clone();
                    checkpoint.cwd = invocation.cwd.display().to_string();
                    Ok(())
                },
            )
            .map_err(|error| match error {
                HandoffStoreError::LeaseConflict => SideAgentError::AlreadyRunning,
                other => SideAgentError::Store(other),
            })?;
        if created_conversation {
            record_audit(
                self.store,
                &env.handoff_id,
                CollaborativeAuditKind::SideConversationCreated,
            );
        }
        record_audit(
            self.store,
            &env.handoff_id,
            CollaborativeAuditKind::SideAgentStarted,
        );
        let handoff = self.store.load_handoff(&env.handoff_id)?;
        let checkpoint = self.store.load_checkpoint(&env.handoff_id)?;
        let observation_start = handoff.shell_log_end.unwrap_or(handoff.shell_log_start);
        let observation = self.observer.observe(&invocation.cwd, observation_start);
        let control_returned = was_waiting.then(|| HumanControlReturned {
            pending_request: handoff.pending_human_request.clone().unwrap_or_default(),
            shell_log_delta: format!(
                "{}..{}",
                observation_start,
                observation.shell_log_end.unwrap_or(observation_start)
            ),
            current_cwd: observation.cwd.clone(),
            current_observation: serde_json::to_string(&observation)
                .unwrap_or_else(|_| "{}".into()),
            user_note: invocation.user_note.clone(),
        });
        Ok(SideAgentDispatch::Run(SideTurn {
            handoff_id: handoff.id.clone(),
            conversation_id,
            system_instruction: ParentCollaborationContextBuilder::build(
                &handoff,
                &checkpoint,
                &observation,
                options.memory_prompt_block,
            ),
            control_returned,
            collaborative_handoff: false,
        }))
    }

    pub fn request_human_action(
        &self,
        handoff_id: &str,
        request: RequestHumanAction,
        tool_call_id: Option<&str>,
    ) -> Result<(), SideAgentError> {
        let mut added_candidates = Vec::new();
        let mut seen_commands = std::collections::HashSet::new();
        for command in &request.command_candidates {
            if !seen_commands.insert(command.clone()) {
                continue;
            }
            let candidate = CommandCandidate {
                id: deterministic_candidate_id(handoff_id, command),
                command: command.clone(),
                description: Some(request.instruction.clone()),
                source: CommandCandidateSource::SideAgent,
                source_run_id: None,
                target_handoff_id: handoff_id.into(),
                created_at_ms: self.runtime.now_ms(),
            };
            added_candidates.push(candidate);
        }
        let handoff = self.store.load_handoff(handoff_id)?;
        let side_conversation_id = handoff.side_conversation_id.clone();
        for candidate in &mut added_candidates {
            candidate.source_run_id = side_conversation_id.clone();
        }
        let observation_start = handoff.shell_log_end.unwrap_or(handoff.shell_log_start);
        let checkpoint = self.store.load_checkpoint(handoff_id)?;
        let observation = self
            .observer
            .observe(Path::new(&checkpoint.cwd), observation_start);
        let pending_human_request = format!(
            "{}\nReason: {}\nExpected completion: {}",
            request.instruction, request.reason, request.expected_completion
        );
        let now = self.runtime.now_ms();
        let summary_event = "side agent requested human action";
        self.store.finish_side_run_atomically(
            handoff_id,
            now,
            &added_candidates,
            tool_call_id,
            &mut |handoff, checkpoint| {
                handoff.state = transition(handoff.state, HandoffEvent::SideAgentWaiting)
                    .map_err(|error| HandoffStoreError::Write(error.to_string()))?;
                handoff.pending_human_request = Some(pending_human_request.clone());
                handoff.conversation_summary =
                    update_summary(&handoff.conversation_summary, summary_event);
                handoff.shell_log_end = observation.shell_log_end;
                checkpoint.control_state = handoff.state;
                checkpoint.conversation_summary = handoff.conversation_summary.clone();
                for candidate in &added_candidates {
                    if checkpoint
                        .command_candidates
                        .iter()
                        .any(|existing| existing.id == candidate.id)
                    {
                        continue;
                    }
                    checkpoint.command_candidates.push(candidate.clone());
                }
                Ok(())
            },
        )?;
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::SideAgentWaitingForHuman,
        );
        Ok(())
    }

    pub fn finish_side_turn(&self, handoff_id: &str, summary: &str) -> Result<(), SideAgentError> {
        let now = self.runtime.now_ms();
        self.store.finish_side_run_atomically(
            handoff_id,
            now,
            &[],
            None,
            &mut |handoff, checkpoint| {
                handoff.state = transition(handoff.state, HandoffEvent::SideAgentReturned)
                    .map_err(|error| HandoffStoreError::Write(error.to_string()))?;
                handoff.conversation_summary =
                    update_summary(&handoff.conversation_summary, summary);
                checkpoint.control_state = handoff.state;
                checkpoint.conversation_summary = handoff.conversation_summary.clone();
                Ok(())
            },
        )?;
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::SideAgentReturned,
        );
        Ok(())
    }
}

pub struct ReadCollaborativeStatus<'a, S> {
    store: &'a S,
}

impl<'a, S> ReadCollaborativeStatus<'a, S>
where
    S: HandoffRepository + CommandCandidateStore,
{
    pub fn new(store: &'a S) -> Self {
        Self { store }
    }

    pub fn read(&self) -> Result<Vec<CollaborativeHandoffReport>, SideAgentError> {
        let mut reports = Vec::new();
        for handoff in self.store.list_handoffs()? {
            if matches!(
                handoff.state,
                HandoffState::Completed | HandoffState::Cancelled
            ) {
                continue;
            }
            let command_candidates = self
                .store
                .list_candidates(&handoff.id)?
                .into_iter()
                .map(|candidate| candidate.command)
                .collect();
            let resume_hint = match handoff.state {
                HandoffState::Orphaned | HandoffState::Returned => {
                    format!("ai resume {}", handoff.id)
                }
                HandoffState::SideAgentRunning => "side agent already running".into(),
                HandoffState::SideAgentWaitingForHuman => "run `ai` to resume side agent".into(),
                _ => "continue in the human shell".into(),
            };
            reports.push(CollaborativeHandoffReport {
                handoff_id: handoff.id,
                parent_task: handoff.parent_request_summary,
                state: handoff_state_label(handoff.state).into(),
                command_candidates,
                resume_hint,
            });
        }
        Ok(reports)
    }
}

fn handoff_state_label(state: HandoffState) -> &'static str {
    match state {
        HandoffState::Creating => "CREATING",
        HandoffState::HumanActive => "HUMAN_ACTIVE",
        HandoffState::SideAgentRunning => "SIDE_AGENT_RUNNING",
        HandoffState::SideAgentWaitingForHuman => "SIDE_AGENT_WAITING_FOR_HUMAN",
        HandoffState::Returned => "RETURNED",
        HandoffState::Orphaned => "ORPHANED",
        HandoffState::ResumingParent => "RESUMING_PARENT",
        HandoffState::Completed => "COMPLETED",
        HandoffState::Cancelled => "CANCELLED",
    }
}

fn try_recover_stale_side_agent<S, R>(
    store: &S,
    runtime: &R,
    handoff_id: &str,
    handoff: &mut Handoff,
    checkpoint: &mut crate::domain::HandoffCheckpoint,
) -> Result<bool, SideAgentError>
where
    S: SideRunLockRepository + CheckpointRepository + HandoffRepository,
    R: HandoffRuntime,
{
    if handoff.state != HandoffState::SideAgentRunning {
        return Ok(false);
    }
    let mut update = |handoff: &mut Handoff, checkpoint: &mut crate::domain::HandoffCheckpoint| {
        handoff.state =
            crate::domain::try_transition(handoff.state, HandoffEvent::SideAgentReturned)
                .map_err(|error| HandoffStoreError::Write(error.to_string()))?;
        handoff.conversation_summary = update_summary(
            &handoff.conversation_summary,
            "side agent process lost; recovered to human control",
        );
        checkpoint.control_state = handoff.state;
        checkpoint.conversation_summary = handoff.conversation_summary.clone();
        Ok(())
    };
    let recovered = store
        .recover_stale_side_agent_run(
            handoff_id,
            &|pid| runtime.process_is_alive(pid),
            runtime.now_ms(),
            &mut update,
        )
        .map_err(SideAgentError::Store)?;
    if recovered {
        *handoff = store.load_handoff(handoff_id)?;
        *checkpoint = store.load_checkpoint(handoff_id)?;
    }
    Ok(recovered)
}

fn validate_identity(metadata: &str, host: &str, uid: u32) -> Result<(), SideAgentError> {
    let value: serde_json::Value =
        serde_json::from_str(metadata).map_err(|_| SideAgentError::InvalidIdentityMetadata)?;
    let expected_host = value
        .get("handoff_host_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or(SideAgentError::InvalidIdentityMetadata)?;
    let expected_uid = value
        .get("handoff_uid")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(SideAgentError::InvalidIdentityMetadata)?;
    if expected_host != host {
        return Err(SideAgentError::HostMismatch);
    }
    if expected_uid != uid {
        return Err(SideAgentError::UidMismatch);
    }
    Ok(())
}

fn transition(state: HandoffState, event: HandoffEvent) -> Result<HandoffState, SideAgentError> {
    try_transition(state, event).map_err(|error| SideAgentError::Transition(error.to_string()))
}

fn update_summary(current: &str, event: &str) -> String {
    if current.trim().is_empty() {
        event.into()
    } else {
        format!("{current}\n- {event}")
    }
}

pub struct ParentCollaborationContextBuilder;

impl ParentCollaborationContextBuilder {
    pub fn build(
        handoff: &Handoff,
        checkpoint: &crate::domain::HandoffCheckpoint,
        observation: &crate::ports::outbound::EnvironmentObservation,
        memory_prompt_block: Option<&str>,
    ) -> String {
        let metadata = serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
            .unwrap_or_else(|_| serde_json::json!({}));
        let work_stage_and_plan = metadata
            .get("work_stage_and_plan")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or(handoff.parent_request_summary.as_str());
        let parent_work_id = handoff
            .parent_goal_id
            .as_deref()
            .or_else(|| {
                metadata
                    .get("parent_work_id")
                    .and_then(|value| value.as_str())
            })
            .unwrap_or("unknown");
        let contextual_memory = memory_prompt_block
            .filter(|block| !block.trim().is_empty())
            .unwrap_or("(not available)");
        format!(
        "You are the side agent for collaborative human handoff.\n\
Do not start a collaborative handoff or a nested human shell. shell_exec runs normally for this side agent.\n\
When human action is required, call the client tool `aish.request_human_action` with instruction, reason, command_candidates, and expected_completion.\n\
handoff_id: {}\nparent_work_id: {}\nparent_task_goal: {}\nwork_stage_and_plan: {}\n\
parent_request: {}\nrequested_operation: {:?}\ncompletion_condition: {}\n\
parent_conversation_summary: {}\nrecent_parent_context: {}\ncontextual_memory_child_goal: {}\n\
contextual_memory: {}\ncwd: {}\nshell_log_start: {}\nreplay_reference: {}\n",
        handoff.id,
        parent_work_id,
        checkpoint.parent_goal,
        work_stage_and_plan,
        handoff.parent_request_summary,
        checkpoint.pending_shell_exec,
        handoff.pending_human_request.as_deref().unwrap_or(&handoff.parent_request_summary),
        handoff.conversation_summary,
        checkpoint.conversation_snapshot,
        checkpoint.child_goal.id,
        contextual_memory,
        observation.cwd,
        checkpoint.shell_log_start,
        handoff.conversation_snapshot_ref,
        )
    }
}

/// 表示専用: assistant テキストが legacy JSON envelope の場合にパースする。
/// 制御経路は client tool `aish.request_human_action` を使用する。
fn try_parse_request_human_action_json_for_display(content: &str) -> Option<RequestHumanAction> {
    #[derive(serde::Deserialize)]
    struct Envelope {
        request_human_action: RequestHumanAction,
    }
    serde_json::from_str::<Envelope>(content.trim())
        .ok()
        .map(|envelope| envelope.request_human_action)
}

/// side agent の `request_human_action` をターミナル向けに整形する（JSON は出さない）。
pub fn format_request_human_action_for_user(request: &RequestHumanAction) -> String {
    let mut lines = vec![
        "ai: side agent があなたの操作を待っています。".into(),
        String::new(),
        format!("依頼: {}", request.instruction),
        format!("理由: {}", request.reason),
    ];
    if !request.expected_completion.is_empty() {
        lines.push(format!("完了の目安: {}", request.expected_completion));
    }
    if !request.command_candidates.is_empty() {
        lines.push(String::new());
        lines.push("候補 (Alt+. / Alt+,):".into());
        for command in &request.command_candidates {
            lines.push(format!("  {command}"));
        }
    }
    lines.push(String::new());
    lines.push("side agent 再開: ai  または  ai <補足>".into());
    lines.push("親へ戻る: Ctrl+D".into());
    lines.join("\n")
}

pub fn presenter_output_for_assistant_content(content: &str) -> Option<(String, bool)> {
    try_parse_request_human_action_json_for_display(content)
        .map(|request| (format_request_human_action_for_user(&request), true))
}
