//! Collaborative handoff の単一 durable workflow aggregate（0055 §33）。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{
    try_transition, ChildGoalCloseReason, CollaborativeAuditKind, Handoff, HandoffCheckpoint,
    HandoffEvent,
};

pub const COLLABORATIVE_WORKFLOW_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborativeWorkflow {
    pub schema_version: u32,
    pub revision: u64,
    pub handoff: Handoff,
    pub checkpoint: HandoffCheckpoint,
    #[serde(default)]
    pub pending_effects: Vec<PendingWorkflowEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWorkflowEffect {
    pub id: String,
    pub kind: WorkflowEffectKind,
    pub state: WorkflowEffectState,
    pub attempts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowEffectKind {
    CreateChildWork,
    CloseChildWork { reason: ChildGoalCloseReason },
    PublishCandidate { candidate_id: String },
    AcquireLease,
    LaunchHumanShell { generation: u32 },
    RemoveCandidateCache,
    ReleaseLease,
    InvalidateShellSession { generation: u32 },
    ResumeParent,
    RecordAudit { event: CollaborativeAuditKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowEffectState {
    Pending,
    InFlight,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollaborativeWorkflowEvent {
    Transition(HandoffEvent),
    ReplaceCheckpoint(Box<HandoffCheckpoint>),
    EnqueueEffect(PendingWorkflowEffect),
    ClaimEffect {
        effect_id: String,
        now_ms: u64,
    },
    CompleteEffect {
        effect_id: String,
        now_ms: u64,
    },
    RetryEffect {
        effect_id: String,
        error: String,
        now_ms: u64,
    },
    FailEffect {
        effect_id: String,
        error: String,
        now_ms: u64,
    },
    RecoverStaleClaims {
        stale_before_ms: u64,
        now_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CollaborativeWorkflowError {
    #[error("workflow id does not match handoff/checkpoint ids")]
    IdentityMismatch,
    #[error("handoff and checkpoint control states differ")]
    ControlStateMismatch,
    #[error("workflow contains duplicate effect id: {0}")]
    DuplicateEffectId(String),
    #[error("workflow effect not found: {0}")]
    EffectNotFound(String),
    #[error("invalid workflow effect transition for {0}")]
    InvalidEffectTransition(String),
    #[error("invalid handoff transition: {0}")]
    InvalidHandoffTransition(String),
    #[error("workflow revision overflow")]
    RevisionOverflow,
}

impl CollaborativeWorkflow {
    pub fn new(
        handoff: Handoff,
        checkpoint: HandoffCheckpoint,
    ) -> Result<Self, CollaborativeWorkflowError> {
        let workflow = Self {
            schema_version: COLLABORATIVE_WORKFLOW_SCHEMA_VERSION,
            revision: 0,
            handoff,
            checkpoint,
            pending_effects: Vec::new(),
        };
        workflow.validate()?;
        Ok(workflow)
    }

    pub fn id(&self) -> &str {
        &self.handoff.id
    }

    pub fn validate(&self) -> Result<(), CollaborativeWorkflowError> {
        if self.handoff.id != self.checkpoint.handoff_id
            || self.handoff.child_goal_id != self.checkpoint.child_goal.id
            || self.checkpoint.child_goal.handoff_id != self.handoff.id
        {
            return Err(CollaborativeWorkflowError::IdentityMismatch);
        }
        if self.handoff.state != self.checkpoint.control_state {
            return Err(CollaborativeWorkflowError::ControlStateMismatch);
        }
        let mut ids = HashSet::new();
        for effect in &self.pending_effects {
            if effect.id.is_empty() || !ids.insert(effect.id.clone()) {
                return Err(CollaborativeWorkflowError::DuplicateEffectId(
                    effect.id.clone(),
                ));
            }
        }
        Ok(())
    }

    pub fn apply(
        &mut self,
        event: CollaborativeWorkflowEvent,
    ) -> Result<(), CollaborativeWorkflowError> {
        let mut next = self.clone();
        next.reduce(event)?;
        next.validate()?;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or(CollaborativeWorkflowError::RevisionOverflow)?;
        *self = next;
        Ok(())
    }

    fn reduce(
        &mut self,
        event: CollaborativeWorkflowEvent,
    ) -> Result<(), CollaborativeWorkflowError> {
        match event {
            CollaborativeWorkflowEvent::Transition(event) => {
                let state = try_transition(self.handoff.state, event).map_err(|error| {
                    CollaborativeWorkflowError::InvalidHandoffTransition(error.to_string())
                })?;
                self.handoff.state = state;
                self.checkpoint.control_state = state;
            }
            CollaborativeWorkflowEvent::ReplaceCheckpoint(checkpoint) => {
                self.checkpoint = *checkpoint;
            }
            CollaborativeWorkflowEvent::EnqueueEffect(effect) => {
                if let Some(existing) = self
                    .pending_effects
                    .iter()
                    .find(|existing| existing.id == effect.id)
                {
                    if existing.kind == effect.kind {
                        return Ok(());
                    }
                    return Err(CollaborativeWorkflowError::DuplicateEffectId(effect.id));
                }
                self.pending_effects.push(effect);
            }
            CollaborativeWorkflowEvent::ClaimEffect { effect_id, now_ms } => {
                let effect = self.effect_mut(&effect_id)?;
                if effect.state != WorkflowEffectState::Pending {
                    return Err(CollaborativeWorkflowError::InvalidEffectTransition(
                        effect_id,
                    ));
                }
                effect.state = WorkflowEffectState::InFlight;
                effect.attempts = effect.attempts.saturating_add(1);
                effect.claimed_at_ms = Some(now_ms);
                effect.updated_at_ms = now_ms;
            }
            CollaborativeWorkflowEvent::CompleteEffect { effect_id, now_ms } => {
                let effect = self.effect_mut(&effect_id)?;
                if effect.state != WorkflowEffectState::InFlight {
                    return Err(CollaborativeWorkflowError::InvalidEffectTransition(
                        effect_id,
                    ));
                }
                effect.state = WorkflowEffectState::Completed;
                effect.claimed_at_ms = None;
                effect.last_error = None;
                effect.updated_at_ms = now_ms;
            }
            CollaborativeWorkflowEvent::RetryEffect {
                effect_id,
                error,
                now_ms,
            } => {
                let effect = self.effect_mut(&effect_id)?;
                if effect.state != WorkflowEffectState::InFlight {
                    return Err(CollaborativeWorkflowError::InvalidEffectTransition(
                        effect_id,
                    ));
                }
                effect.state = WorkflowEffectState::Pending;
                effect.claimed_at_ms = None;
                effect.last_error = Some(sanitize_workflow_effect_error(&error));
                effect.updated_at_ms = now_ms;
            }
            CollaborativeWorkflowEvent::FailEffect {
                effect_id,
                error,
                now_ms,
            } => {
                let effect = self.effect_mut(&effect_id)?;
                if effect.state != WorkflowEffectState::InFlight {
                    return Err(CollaborativeWorkflowError::InvalidEffectTransition(
                        effect_id,
                    ));
                }
                effect.state = WorkflowEffectState::Failed;
                effect.claimed_at_ms = None;
                effect.last_error = Some(sanitize_workflow_effect_error(&error));
                effect.updated_at_ms = now_ms;
            }
            CollaborativeWorkflowEvent::RecoverStaleClaims {
                stale_before_ms,
                now_ms,
            } => {
                for effect in &mut self.pending_effects {
                    if effect.state == WorkflowEffectState::InFlight
                        && effect.claimed_at_ms.is_some_and(|at| at <= stale_before_ms)
                    {
                        effect.state = WorkflowEffectState::Pending;
                        effect.claimed_at_ms = None;
                        effect.last_error = Some("recovered stale effect claim".into());
                        effect.updated_at_ms = now_ms;
                    }
                }
            }
        }
        Ok(())
    }

    fn effect_mut(
        &mut self,
        effect_id: &str,
    ) -> Result<&mut PendingWorkflowEffect, CollaborativeWorkflowError> {
        self.pending_effects
            .iter_mut()
            .find(|effect| effect.id == effect_id)
            .ok_or_else(|| CollaborativeWorkflowError::EffectNotFound(effect_id.into()))
    }
}

impl PendingWorkflowEffect {
    pub fn pending(id: impl Into<String>, kind: WorkflowEffectKind, now_ms: u64) -> Self {
        Self {
            id: id.into(),
            kind,
            state: WorkflowEffectState::Pending,
            attempts: 0,
            claimed_at_ms: None,
            last_error: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }
}

/// effect 永続化用に外部エラーを安全な分類コードへ変換する（原文・秘密値は保存しない）。
pub fn sanitize_workflow_effect_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("not found") {
        return "resource_not_found".into();
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return "timeout".into();
    }
    if lower.contains("lease") || lower.contains("conflict") {
        return "conflict".into();
    }
    if lower.contains("permission") || lower.contains("denied") {
        return "permission_denied".into();
    }
    if lower.contains("network") || lower.contains("connection") {
        return "network_error".into();
    }
    "effect_failed".into()
}
