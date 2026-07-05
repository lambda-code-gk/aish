//! Human shell 内 `ai` を handoff の side agent へ接続する application service。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::domain::{
    try_transition, CollaborativeAuditKind, CollaborativeHandoffReport, CommandCandidate,
    CommandCandidateSource, Handoff, HandoffEvent, HandoffState,
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RequestHumanAction {
    pub instruction: String,
    pub reason: String,
    pub command_candidates: Vec<String>,
    pub expected_completion: String,
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

    /// spec §13.5 の判定順（standalone → env → token/identity → state）を保つ。
    pub fn dispatch(
        &self,
        env: Option<CollaborativeShellEnvironment>,
        invocation: &SideAgentInvocation,
    ) -> Result<SideAgentDispatch, SideAgentError> {
        if invocation.standalone {
            return Ok(SideAgentDispatch::Standalone);
        }
        let Some(env) = env else {
            return Ok(SideAgentDispatch::Normal);
        };
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
                return Ok(SideAgentDispatch::PromptForInput {
                    handoff_id: handoff.id,
                });
            }
            HandoffState::SideAgentRunning => return Err(SideAgentError::AlreadyRunning),
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

        self.store.try_acquire_side_run_lock(
            &env.handoff_id,
            &LeaseAcquireRequest {
                owner_client_id: invocation.client_id.clone(),
                owner_process_id: invocation.process_id,
                owner_tty: invocation.tty.clone(),
                owner_host: self.runtime.host_id(),
                owner_uid: self.runtime.effective_uid(),
                now_ms: self.runtime.now_ms(),
                lease_timeout_ms: 120_000,
            },
        )?;

        let conversation_id = match handoff.side_conversation_id.clone() {
            Some(id) => id,
            None => {
                let id = self.runtime.unique_id("side-conversation");
                handoff.side_conversation_id = Some(id.clone());
                checkpoint.side_conversation_id = Some(id.clone());
                record_audit(
                    self.store,
                    &env.handoff_id,
                    CollaborativeAuditKind::SideConversationCreated,
                );
                id
            }
        };
        let was_waiting = handoff.state == HandoffState::SideAgentWaitingForHuman;
        handoff.state = transition(
            handoff.state,
            if was_waiting {
                HandoffEvent::SideAgentResumed
            } else {
                HandoffEvent::StartSideAgent
            },
        )?;
        handoff.conversation_summary = update_summary(
            &handoff.conversation_summary,
            if was_waiting {
                "human control returned; side run resumed"
            } else {
                "side run started"
            },
        );
        handoff.updated_at_ms = self.runtime.now_ms();
        checkpoint.control_state = handoff.state;
        checkpoint.conversation_summary = handoff.conversation_summary.clone();
        checkpoint.cwd = invocation.cwd.display().to_string();
        self.store.save_checkpoint(&handoff.id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        record_audit(
            self.store,
            &handoff.id,
            CollaborativeAuditKind::SideAgentStarted,
        );

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
            ),
            control_returned,
            collaborative_handoff: false,
        }))
    }

    pub fn request_human_action(
        &self,
        handoff_id: &str,
        request: RequestHumanAction,
    ) -> Result<(), SideAgentError> {
        let mut handoff = self.store.load_handoff(handoff_id)?;
        handoff.state = transition(handoff.state, HandoffEvent::SideAgentWaiting)?;
        handoff.pending_human_request = Some(format!(
            "{}\nReason: {}\nExpected completion: {}",
            request.instruction, request.reason, request.expected_completion
        ));
        handoff.conversation_summary = update_summary(
            &handoff.conversation_summary,
            "side agent requested human action",
        );
        handoff.updated_at_ms = self.runtime.now_ms();
        let mut added_candidates = Vec::new();
        for command in request.command_candidates {
            let candidate = CommandCandidate {
                id: self.runtime.unique_id("candidate"),
                command,
                description: Some(request.instruction.clone()),
                source: CommandCandidateSource::SideAgent,
                source_run_id: handoff.side_conversation_id.clone(),
                target_handoff_id: handoff_id.into(),
                created_at_ms: self.runtime.now_ms(),
            };
            self.store.append_candidate(handoff_id, &candidate)?;
            added_candidates.push(candidate);
        }
        let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
        let observation_start = handoff.shell_log_end.unwrap_or(handoff.shell_log_start);
        let observation = self
            .observer
            .observe(Path::new(&checkpoint.cwd), observation_start);
        handoff.shell_log_end = observation.shell_log_end;
        checkpoint.control_state = handoff.state;
        checkpoint.conversation_summary = handoff.conversation_summary.clone();
        checkpoint.command_candidates.extend(added_candidates);
        self.store.save_checkpoint(handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::SideAgentWaitingForHuman,
        );
        self.store.release_side_run_lock(handoff_id)?;
        Ok(())
    }

    pub fn finish_side_turn(&self, handoff_id: &str, summary: &str) -> Result<(), SideAgentError> {
        let mut handoff = self.store.load_handoff(handoff_id)?;
        handoff.state = transition(handoff.state, HandoffEvent::SideAgentReturned)?;
        handoff.conversation_summary = update_summary(&handoff.conversation_summary, summary);
        handoff.updated_at_ms = self.runtime.now_ms();
        let mut checkpoint = self.store.load_checkpoint(handoff_id)?;
        checkpoint.control_state = handoff.state;
        checkpoint.conversation_summary = handoff.conversation_summary.clone();
        self.store.save_checkpoint(handoff_id, &checkpoint)?;
        self.store.save_handoff(&handoff)?;
        record_audit(
            self.store,
            handoff_id,
            CollaborativeAuditKind::SideAgentReturned,
        );
        self.store.release_side_run_lock(handoff_id)?;
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
    ) -> String {
        format!(
        "You are the side agent for collaborative human handoff.\n\
Do not start a collaborative handoff or a nested human shell. shell_exec runs normally for this side agent.\n\
handoff_id: {}\nparent_task_goal: {}\nwork_stage_and_plan: {}\n\
parent_request: {}\nrequested_operation: {:?}\ncompletion_condition: {}\n\
parent_conversation_summary: {}\nrecent_parent_context: {}\ncontextual_memory_child_goal: {}\n\
cwd: {}\nshell_log_start: {}\nreplay_reference: {}\n\
        When human action is required, respond with a single JSON object only (no markdown fences): {{\"request_human_action\":{{\"instruction\":\"...\",\"reason\":\"...\",\"command_candidates\":[],\"expected_completion\":\"...\"}}}}. The user sees a formatted summary, not the raw JSON.",
        handoff.id,
        checkpoint.parent_goal,
        handoff.parent_request_summary,
        handoff.pending_human_request.as_deref().unwrap_or_default(),
        checkpoint.pending_shell_exec,
        handoff.pending_human_request.as_deref().unwrap_or_default(),
        handoff.conversation_summary,
        checkpoint.conversation_snapshot,
        checkpoint.child_goal.id,
        observation.cwd,
        checkpoint.shell_log_start,
        handoff.conversation_snapshot_ref,
        )
    }
}

pub fn parse_request_human_action(content: &str) -> Option<RequestHumanAction> {
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
    parse_request_human_action(content)
        .map(|request| (format_request_human_action_for_user(&request), true))
}
