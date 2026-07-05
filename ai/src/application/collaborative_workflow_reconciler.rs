//! Collaborative workflow の pending effect を収束させる単一 reconciler。

use crate::domain::{
    sanitize_workflow_effect_error, CollaborativeWorkflow, CollaborativeWorkflowError,
    CollaborativeWorkflowEvent, PendingWorkflowEffect, WorkflowEffectState,
};
use crate::ports::outbound::{CollaborativeWorkflowRepository, HandoffStoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowEffectError {
    pub message: String,
    pub retryable: bool,
}

pub trait CollaborativeWorkflowEffectExecutor: Send {
    /// effect ID を外部 API の冪等キーとして扱うこと。
    fn execute(
        &self,
        workflow: &CollaborativeWorkflow,
        effect: &PendingWorkflowEffect,
    ) -> Result<(), WorkflowEffectError>;
}

pub trait CollaborativeWorkflowClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    pub handoff_id: String,
    pub completed_effects: Vec<String>,
    pub retryable_effects: Vec<String>,
    pub failed_effects: Vec<String>,
}

pub struct CollaborativeWorkflowReconciler<'a, S, E, C> {
    store: &'a S,
    executor: &'a E,
    clock: &'a C,
    claim_timeout_ms: u64,
}

impl<'a, S, E, C> CollaborativeWorkflowReconciler<'a, S, E, C>
where
    S: CollaborativeWorkflowRepository,
    E: CollaborativeWorkflowEffectExecutor,
    C: CollaborativeWorkflowClock,
{
    pub fn new(store: &'a S, executor: &'a E, clock: &'a C, claim_timeout_ms: u64) -> Self {
        Self {
            store,
            executor,
            clock,
            claim_timeout_ms,
        }
    }

    pub fn reconcile_all(&self) -> Result<Vec<ReconcileReport>, HandoffStoreError> {
        self.store
            .list_workflows()?
            .into_iter()
            .map(|workflow| self.reconcile(&workflow.handoff.id))
            .collect()
    }

    pub fn reconcile(&self, handoff_id: &str) -> Result<ReconcileReport, HandoffStoreError> {
        let now = self.clock.now_ms();
        let stale_before = now.saturating_sub(self.claim_timeout_ms);
        self.store.mutate_workflow(handoff_id, &mut |workflow| {
            workflow
                .apply(CollaborativeWorkflowEvent::RecoverStaleClaims {
                    stale_before_ms: stale_before,
                    now_ms: now,
                })
                .map_err(HandoffStoreError::from)
        })?;

        // 1 invocation につき各 effect は最大1回。retryable failure の busy loop を防ぐ。
        let effect_ids: Vec<String> = self
            .store
            .load_workflow(handoff_id)?
            .pending_effects
            .iter()
            .filter(|effect| effect.state == WorkflowEffectState::Pending)
            .map(|effect| effect.id.clone())
            .collect();
        let mut report = ReconcileReport {
            handoff_id: handoff_id.into(),
            completed_effects: Vec::new(),
            retryable_effects: Vec::new(),
            failed_effects: Vec::new(),
        };

        for effect_id in effect_ids {
            let claimed = match self.store.mutate_workflow(handoff_id, &mut |workflow| {
                workflow
                    .apply(CollaborativeWorkflowEvent::ClaimEffect {
                        effect_id: effect_id.clone(),
                        now_ms: self.clock.now_ms(),
                    })
                    .map_err(HandoffStoreError::from)
            }) {
                Ok(workflow) => workflow,
                Err(HandoffStoreError::InvalidWorkflow(
                    CollaborativeWorkflowError::InvalidEffectTransition(_),
                )) => continue,
                Err(error) => return Err(error),
            };
            let Some(effect) = claimed
                .pending_effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .cloned()
            else {
                return Err(HandoffStoreError::Write(
                    "claimed workflow effect disappeared".into(),
                ));
            };
            match self.executor.execute(&claimed, &effect) {
                Ok(()) => {
                    self.finish_effect(handoff_id, &effect_id, None, false)?;
                    report.completed_effects.push(effect_id);
                }
                Err(error) if error.retryable => {
                    self.finish_effect(
                        handoff_id,
                        &effect_id,
                        Some(sanitize_workflow_effect_error(&error.message)),
                        false,
                    )?;
                    report.retryable_effects.push(effect_id);
                }
                Err(error) => {
                    self.finish_effect(
                        handoff_id,
                        &effect_id,
                        Some(sanitize_workflow_effect_error(&error.message)),
                        true,
                    )?;
                    report.failed_effects.push(effect_id);
                }
            }
        }
        Ok(report)
    }

    fn finish_effect(
        &self,
        handoff_id: &str,
        effect_id: &str,
        error: Option<String>,
        terminal: bool,
    ) -> Result<(), HandoffStoreError> {
        self.store.mutate_workflow(handoff_id, &mut |workflow| {
            let event = match error.clone() {
                None => CollaborativeWorkflowEvent::CompleteEffect {
                    effect_id: effect_id.into(),
                    now_ms: self.clock.now_ms(),
                },
                Some(error) if terminal => CollaborativeWorkflowEvent::FailEffect {
                    effect_id: effect_id.into(),
                    error,
                    now_ms: self.clock.now_ms(),
                },
                Some(error) => CollaborativeWorkflowEvent::RetryEffect {
                    effect_id: effect_id.into(),
                    error,
                    now_ms: self.clock.now_ms(),
                },
            };
            workflow.apply(event).map_err(HandoffStoreError::from)
        })?;
        Ok(())
    }
}
