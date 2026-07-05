//! Application service から aggregate を一括更新するための共通入口。

use crate::domain::{
    sanitize_workflow_effect_error, CollaborativeWorkflow, CollaborativeWorkflowEvent, Handoff,
    HandoffCheckpoint, PendingWorkflowEffect,
};
use crate::ports::outbound::{CollaborativeWorkflowRepository, HandoffStoreError};

pub(crate) fn create_workflow<S: CollaborativeWorkflowRepository>(
    store: &S,
    handoff: Handoff,
    checkpoint: HandoffCheckpoint,
    effects: Vec<PendingWorkflowEffect>,
) -> Result<CollaborativeWorkflow, HandoffStoreError> {
    let mut workflow = CollaborativeWorkflow::new(handoff, checkpoint)?;
    for effect in effects {
        workflow.apply(CollaborativeWorkflowEvent::EnqueueEffect(effect))?;
    }
    store.create_workflow(&workflow)?;
    Ok(workflow)
}

/// CAS 安全な workflow mutation。再試行時も closure 内で最新 aggregate を更新する。
pub(crate) fn update_workflow<S, F>(
    store: &S,
    handoff_id: &str,
    mut f: F,
) -> Result<CollaborativeWorkflow, HandoffStoreError>
where
    S: CollaborativeWorkflowRepository,
    F: FnMut(&mut CollaborativeWorkflow) -> Result<(), HandoffStoreError>,
{
    store.mutate_workflow(handoff_id, &mut f)
}

pub(crate) fn claim_effect<S: CollaborativeWorkflowRepository>(
    store: &S,
    handoff_id: &str,
    effect_id: &str,
    now_ms: u64,
) -> Result<(), HandoffStoreError> {
    update_workflow(store, handoff_id, |workflow| {
        workflow
            .apply(CollaborativeWorkflowEvent::ClaimEffect {
                effect_id: effect_id.into(),
                now_ms,
            })
            .map_err(HandoffStoreError::from)
    })?;
    Ok(())
}

pub(crate) fn complete_effect<S: CollaborativeWorkflowRepository>(
    store: &S,
    handoff_id: &str,
    effect_id: &str,
    now_ms: u64,
) -> Result<(), HandoffStoreError> {
    update_workflow(store, handoff_id, |workflow| {
        workflow
            .apply(CollaborativeWorkflowEvent::CompleteEffect {
                effect_id: effect_id.into(),
                now_ms,
            })
            .map_err(HandoffStoreError::from)
    })?;
    Ok(())
}

pub(crate) fn retry_effect<S: CollaborativeWorkflowRepository>(
    store: &S,
    handoff_id: &str,
    effect_id: &str,
    error: &str,
    now_ms: u64,
) -> Result<(), HandoffStoreError> {
    let safe = sanitize_workflow_effect_error(error);
    update_workflow(store, handoff_id, |workflow| {
        workflow
            .apply(CollaborativeWorkflowEvent::RetryEffect {
                effect_id: effect_id.into(),
                error: safe.clone(),
                now_ms,
            })
            .map_err(HandoffStoreError::from)
    })?;
    Ok(())
}

pub(crate) fn update_handoff<S, F>(
    store: &S,
    handoff_id: &str,
    mut f: F,
) -> Result<(), HandoffStoreError>
where
    S: CollaborativeWorkflowRepository,
    F: FnMut(&mut Handoff) -> Result<(), HandoffStoreError>,
{
    update_workflow(store, handoff_id, |workflow| {
        f(&mut workflow.handoff)?;
        workflow.checkpoint.control_state = workflow.handoff.state;
        Ok(())
    })?;
    Ok(())
}

pub(crate) fn update_checkpoint<S, F>(
    store: &S,
    handoff_id: &str,
    mut f: F,
) -> Result<(), HandoffStoreError>
where
    S: CollaborativeWorkflowRepository,
    F: FnMut(&mut HandoffCheckpoint) -> Result<(), HandoffStoreError>,
{
    update_workflow(store, handoff_id, |workflow| {
        f(&mut workflow.checkpoint)?;
        workflow.handoff.state = workflow.checkpoint.control_state;
        Ok(())
    })?;
    Ok(())
}
