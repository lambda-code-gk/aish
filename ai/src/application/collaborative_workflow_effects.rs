//! 本番経路向け workflow pending effect executor。

use std::path::Path;

use super::child_goal_environment_patch;
use super::collaborative_workflow::update_workflow;
use crate::application::{
    CollaborativeWorkflowEffectExecutor, CollaborativeWorkflowReconciler, ReconcileReport,
    WorkflowEffectError,
};
use crate::domain::{build_candidate_command, CollaborativeWorkflow, WorkflowEffectKind};
use crate::ports::outbound::{
    CollaborativeChildGoalService, CollaborativeWorkflowRepository, HandoffAuditRepository,
    HandoffCandidatePublisher, HandoffRuntime, HandoffStoreError, LeaseRepository,
};

pub struct RuntimeWorkflowClock<'a, R: HandoffRuntime> {
    pub runtime: &'a R,
}

impl<R: HandoffRuntime> crate::application::CollaborativeWorkflowClock
    for RuntimeWorkflowClock<'_, R>
{
    fn now_ms(&self) -> u64 {
        self.runtime.now_ms()
    }
}

pub struct HandoffWorkflowEffectExecutor<'a, S, C: ?Sized, P: ?Sized> {
    store: &'a S,
    child_goal: &'a C,
    candidate_publisher: &'a P,
}

impl<'a, S, C: ?Sized, P: ?Sized> HandoffWorkflowEffectExecutor<'a, S, C, P> {
    pub fn new(store: &'a S, child_goal: &'a C, candidate_publisher: &'a P) -> Self {
        Self {
            store,
            child_goal,
            candidate_publisher,
        }
    }
}

impl<S, C, P> CollaborativeWorkflowEffectExecutor for HandoffWorkflowEffectExecutor<'_, S, C, P>
where
    S: CollaborativeWorkflowRepository + LeaseRepository + HandoffAuditRepository + Sync,
    C: CollaborativeChildGoalService + ?Sized,
    P: HandoffCandidatePublisher + ?Sized,
{
    fn execute(
        &self,
        workflow: &CollaborativeWorkflow,
        effect: &crate::domain::PendingWorkflowEffect,
    ) -> Result<(), WorkflowEffectError> {
        match &effect.kind {
            WorkflowEffectKind::CreateChildWork => {
                let checkpoint = &workflow.checkpoint;
                let mut meta = checkpoint.child_goal.clone();
                if meta.work_mode.is_some() {
                    return Ok(());
                }
                let requested = &checkpoint.pending_shell_exec;
                let candidate_text = build_candidate_command(&requested.command, &requested.args);
                let human_request =
                    format!("次のコマンドを確認し、必要なら実行してください: {candidate_text}");
                let cwd = Path::new(&checkpoint.cwd);
                match self.child_goal.create_child_goal(
                    &mut meta,
                    cwd,
                    &checkpoint.parent_goal,
                    &workflow.handoff.parent_request_summary,
                    &candidate_text,
                    &human_request,
                ) {
                    Ok(()) => {}
                    Err(error) if meta.work_mode.is_none() => {
                        return Err(WorkflowEffectError {
                            message: error.to_string(),
                            retryable: true,
                        });
                    }
                    Err(_) => {}
                }
                let handoff_id = workflow.handoff.id.clone();
                let child_meta = meta.clone();
                update_workflow(self.store, &handoff_id, |workflow| {
                    workflow.checkpoint.child_goal = child_meta.clone();
                    let metadata = child_goal_environment_patch(&workflow.checkpoint);
                    workflow.checkpoint.environment_metadata = metadata.to_string();
                    Ok(())
                })
                .map_err(|error| WorkflowEffectError {
                    message: error.to_string(),
                    retryable: true,
                })?;
                Ok(())
            }
            WorkflowEffectKind::PublishCandidate { candidate_id } => {
                let candidate = workflow
                    .checkpoint
                    .command_candidates
                    .iter()
                    .find(|candidate| candidate.id == *candidate_id)
                    .ok_or_else(|| WorkflowEffectError {
                        message: "candidate not found in checkpoint".into(),
                        retryable: false,
                    })?;
                self.candidate_publisher
                    .publish(
                        &workflow.handoff.id,
                        std::slice::from_ref(&candidate.command),
                    )
                    .map_err(|error| WorkflowEffectError {
                        message: error,
                        retryable: true,
                    })
            }
            WorkflowEffectKind::ReleaseLease => self
                .store
                .release_lease(&workflow.handoff.id)
                .map_err(|error| WorkflowEffectError {
                    message: error.to_string(),
                    retryable: true,
                }),
            WorkflowEffectKind::RecordAudit { event } => self
                .store
                .record_audit(&workflow.handoff.id, *event)
                .map_err(|error| WorkflowEffectError {
                    message: error.to_string(),
                    retryable: true,
                }),
            WorkflowEffectKind::RemoveCandidateCache => self
                .candidate_publisher
                .remove(&workflow.handoff.id)
                .map_err(|error| WorkflowEffectError {
                    message: error,
                    retryable: true,
                }),
            WorkflowEffectKind::AcquireLease
            | WorkflowEffectKind::LaunchHumanShell { .. }
            | WorkflowEffectKind::InvalidateShellSession { .. }
            | WorkflowEffectKind::ResumeParent
            | WorkflowEffectKind::CloseChildWork { .. } => Err(WorkflowEffectError {
                message: "effect requires live process context; resume via operation entrypoint"
                    .into(),
                retryable: false,
            }),
        }
    }
}

pub fn reconcile_pending_workflow_effects<S, P, R, F>(
    store: &S,
    runtime: &R,
    candidate_publisher: &P,
    mut child_goal_for: F,
) -> Result<Vec<ReconcileReport>, HandoffStoreError>
where
    S: CollaborativeWorkflowRepository + LeaseRepository + HandoffAuditRepository + Sync,
    P: HandoffCandidatePublisher + ?Sized,
    R: HandoffRuntime,
    F: FnMut(&str) -> Box<dyn CollaborativeChildGoalService>,
{
    const CLAIM_TIMEOUT_MS: u64 = 120_000;
    let clock = RuntimeWorkflowClock { runtime };
    let mut reports = Vec::new();
    for workflow in store.list_workflows()? {
        let handoff_id = workflow.handoff.id.clone();
        let child_goal = child_goal_for(&handoff_id);
        let executor =
            HandoffWorkflowEffectExecutor::new(store, child_goal.as_ref(), candidate_publisher);
        let reconciler =
            CollaborativeWorkflowReconciler::new(store, &executor, &clock, CLAIM_TIMEOUT_MS);
        reports.push(reconciler.reconcile(&handoff_id)?);
    }
    Ok(reports)
}
