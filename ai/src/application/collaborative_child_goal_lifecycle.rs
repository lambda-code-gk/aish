//! child goal の Work Pop を checkpoint と同期して永続化する。

use std::path::Path;

use crate::domain::{
    child_goal_close_is_conflict, mark_child_goal_closed, ChildGoalCloseReason,
    ChildGoalCloseState, HandoffCheckpoint,
};
use crate::ports::outbound::{
    CheckpointRepository, CollaborativeChildGoalError, CollaborativeChildGoalService,
    HandoffRepository, HandoffStoreError,
};

pub fn persist_child_goal_checkpoint<S: CheckpointRepository>(
    store: &S,
    handoff_id: &str,
    child_goal: &crate::domain::ChildGoalMeta,
) -> Result<(), HandoffStoreError> {
    let mut checkpoint = store.load_checkpoint(handoff_id)?;
    checkpoint.child_goal = child_goal.clone();
    store.save_checkpoint(handoff_id, &checkpoint)
}

pub fn close_child_goal_durable<S, C>(
    store: &S,
    child_goal_service: &C,
    handoff_id: &str,
    reason: ChildGoalCloseReason,
) -> Result<(), CollaborativeChildGoalError>
where
    S: CheckpointRepository + HandoffRepository,
    C: CollaborativeChildGoalService + ?Sized,
{
    let mut checkpoint = store
        .load_checkpoint(handoff_id)
        .map_err(store_err_create)?;
    if checkpoint.child_goal.work_id.is_none() {
        if checkpoint.child_goal.close_state != Some(ChildGoalCloseState::Completed) {
            checkpoint.child_goal.close_state = Some(ChildGoalCloseState::Completed);
            store
                .save_checkpoint(handoff_id, &checkpoint)
                .map_err(store_err_create)?;
        }
        return Ok(());
    }
    if checkpoint.child_goal.close_state == Some(ChildGoalCloseState::Completed) {
        return Ok(());
    }
    checkpoint.child_goal.close_state = Some(ChildGoalCloseState::Pending);
    store
        .save_checkpoint(handoff_id, &checkpoint)
        .map_err(store_err_create)?;

    let cwd = Path::new(&checkpoint.cwd);
    match child_goal_service.close_child_goal(&checkpoint.child_goal, cwd, reason) {
        Ok(()) => {
            let mut checkpoint = store
                .load_checkpoint(handoff_id)
                .map_err(store_err_create)?;
            mark_child_goal_closed(&mut checkpoint.child_goal, reason);
            store
                .save_checkpoint(handoff_id, &checkpoint)
                .map_err(store_err_create)?;
            Ok(())
        }
        Err(error) => {
            let message = error.to_string();
            let mut checkpoint = store.load_checkpoint(handoff_id).map_err(store_err_close)?;
            checkpoint.child_goal.close_state = Some(if child_goal_close_is_conflict(&message) {
                ChildGoalCloseState::Conflict
            } else {
                ChildGoalCloseState::Failed
            });
            store
                .save_checkpoint(handoff_id, &checkpoint)
                .map_err(store_err_close)?;
            let mut handoff = store.load_handoff(handoff_id).map_err(store_err_close)?;
            handoff.resume_error = Some(format!("child_goal_close: {message}"));
            store.save_handoff(&handoff).map_err(store_err_close)?;
            Err(error)
        }
    }
}

pub fn compensate_child_goal_durable<S, C>(
    store: &S,
    child_goal_service: &C,
    handoff_id: &str,
) -> Result<(), CollaborativeChildGoalError>
where
    S: CheckpointRepository + HandoffRepository,
    C: CollaborativeChildGoalService + ?Sized,
{
    close_child_goal_durable(
        store,
        child_goal_service,
        handoff_id,
        ChildGoalCloseReason::Compensated,
    )
}

/// checkpoint 上で未完了の child Work がある場合、Noop ではなく実 service が必要。
pub fn handoff_requires_child_goal_service(checkpoint: &HandoffCheckpoint) -> bool {
    checkpoint.child_goal.work_id.is_some()
        && checkpoint.child_goal.close_state != Some(ChildGoalCloseState::Completed)
}

pub fn child_goal_environment_patch(checkpoint: &HandoffCheckpoint) -> serde_json::Value {
    let mut metadata = serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = metadata.as_object_mut() {
        if let Some(root_id) = checkpoint.child_goal.auto_root_work_id {
            object.insert("auto_root_work_id".into(), root_id.into());
        }
        if let Some(work_id) = checkpoint.child_goal.work_id {
            object.insert("child_work_id".into(), work_id.into());
        }
    }
    metadata
}

fn store_err_create(error: HandoffStoreError) -> CollaborativeChildGoalError {
    CollaborativeChildGoalError::Create(error.to_string())
}

fn store_err_close(error: HandoffStoreError) -> CollaborativeChildGoalError {
    CollaborativeChildGoalError::Close(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ChildGoalAchievement, ChildGoalMeta, HandoffCheckpoint, HandoffState, RequestedShellExec,
    };
    use crate::ports::outbound::{HandoffRepository, HandoffStoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MemStore {
        checkpoints: Mutex<HashMap<String, HandoffCheckpoint>>,
        handoffs: Mutex<HashMap<String, crate::domain::Handoff>>,
    }

    impl MemStore {
        fn with_checkpoint(child_goal: ChildGoalMeta) -> (Self, String) {
            let handoff_id: String = "ho-test".into();
            let checkpoint = HandoffCheckpoint {
                parent_task_id: "task".into(),
                parent_conversation_id: "conv".into(),
                parent_run_id: "run".into(),
                pending_shell_exec: RequestedShellExec {
                    command: "echo".into(),
                    args: vec![],
                    cwd: None,
                    tool_call_id: None,
                },
                parent_goal: "goal".into(),
                child_goal,
                conversation_snapshot: "snap".into(),
                conversation_summary: "summary".into(),
                cwd: "/tmp".into(),
                environment_metadata: "{}".into(),
                handoff_id: handoff_id.clone(),
                side_conversation_id: None,
                command_candidates: vec![],
                shell_log_start: 0,
                control_state: HandoffState::HumanActive,
                provider_metadata: None,
                tool_executions: vec![],
            };
            let store = Self {
                checkpoints: Mutex::new(HashMap::from([(handoff_id.clone(), checkpoint.clone())])),
                handoffs: Mutex::new(HashMap::from([(
                    handoff_id.clone(),
                    crate::domain::Handoff {
                        id: handoff_id.clone(),
                        schema_version: crate::domain::HANDOFF_SCHEMA_VERSION,
                        parent_task_id: checkpoint.parent_task_id.clone(),
                        parent_conversation_id: checkpoint.parent_conversation_id.clone(),
                        parent_run_id: checkpoint.parent_run_id.clone(),
                        parent_goal_id: None,
                        child_goal_id: checkpoint.child_goal.id.clone(),
                        side_conversation_id: None,
                        state: HandoffState::HumanActive,
                        initial_cwd: checkpoint.cwd.clone(),
                        final_shell_cwd: None,
                        parent_request_summary: "test".into(),
                        requested_shell_execs: vec![checkpoint.pending_shell_exec.clone()],
                        pending_human_request: None,
                        conversation_snapshot_ref: "checkpoint.json#conversation_snapshot".into(),
                        conversation_summary: checkpoint.conversation_summary.clone(),
                        checkpoint_ref: "checkpoint.json".into(),
                        before_observation_ref: "{}".into(),
                        after_observation_ref: None,
                        shell_log_start: 0,
                        shell_log_end: None,
                        shell_generation: 1,
                        return_reason: None,
                        human_shell_exit_code: None,
                        resume_error: None,
                        created_at_ms: 1,
                        updated_at_ms: 1,
                    },
                )])),
            };
            (store, handoff_id)
        }
    }

    impl CheckpointRepository for MemStore {
        fn save_checkpoint(
            &self,
            handoff_id: &str,
            checkpoint: &HandoffCheckpoint,
        ) -> Result<(), HandoffStoreError> {
            self.checkpoints
                .lock()
                .unwrap()
                .insert(handoff_id.to_string(), checkpoint.clone());
            Ok(())
        }

        fn load_checkpoint(
            &self,
            handoff_id: &str,
        ) -> Result<HandoffCheckpoint, HandoffStoreError> {
            self.checkpoints
                .lock()
                .unwrap()
                .get(handoff_id)
                .cloned()
                .ok_or_else(|| HandoffStoreError::NotFound(handoff_id.into()))
        }
    }

    impl HandoffRepository for MemStore {
        fn save_handoff(&self, handoff: &crate::domain::Handoff) -> Result<(), HandoffStoreError> {
            self.handoffs
                .lock()
                .unwrap()
                .insert(handoff.id.clone(), handoff.clone());
            Ok(())
        }

        fn load_handoff(
            &self,
            handoff_id: &str,
        ) -> Result<crate::domain::Handoff, HandoffStoreError> {
            self.handoffs
                .lock()
                .unwrap()
                .get(handoff_id)
                .cloned()
                .ok_or_else(|| HandoffStoreError::NotFound(handoff_id.into()))
        }

        fn list_handoffs(&self) -> Result<Vec<crate::domain::Handoff>, HandoffStoreError> {
            Ok(self.handoffs.lock().unwrap().values().cloned().collect())
        }
    }

    struct FailingCloseService;

    impl CollaborativeChildGoalService for FailingCloseService {
        fn create_child_goal(
            &self,
            _meta: &mut ChildGoalMeta,
            _cwd: &Path,
            _parent_goal: &str,
            _handoff_reason: &str,
            _requested_command: &str,
            _human_request: &str,
        ) -> Result<(), CollaborativeChildGoalError> {
            Ok(())
        }

        fn close_child_goal(
            &self,
            _meta: &ChildGoalMeta,
            _cwd: &Path,
            _reason: ChildGoalCloseReason,
        ) -> Result<(), CollaborativeChildGoalError> {
            Err(CollaborativeChildGoalError::Close(
                "active work mismatch: active=99 expected=2".into(),
            ))
        }
    }

    #[test]
    fn close_durable_marks_completed_only_after_pop_success() {
        use crate::ports::outbound::WorkClient;
        use aibe_protocol::{
            ClientResponse, MemoryContext, WorkApplyResponseBody, WorkMutationKindDto,
            WorkMutationOutcomeDto, WorkOperationDto, WorkQueryResponseBody, WorkSnapshotDto,
        };
        use std::sync::Arc;

        struct PopClient {
            popped: Arc<Mutex<bool>>,
        }

        struct PopClosingService {
            client: PopClient,
        }

        impl crate::ports::outbound::WorkClient for PopClient {
            fn work_query(
                &self,
                _session_id: &str,
                _context: &MemoryContext,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                    id: "q".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 1,
                        active_work_id: Some(2),
                        stack: vec![],
                        works: vec![],
                        entries: vec![],
                    },
                }))
            }

            fn work_apply(
                &self,
                _session_id: &str,
                _context: &MemoryContext,
                operation: WorkOperationDto,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                assert!(matches!(operation, WorkOperationDto::Pop));
                *self.popped.lock().unwrap() = true;
                Ok(ClientResponse::WorkApplyResult(WorkApplyResponseBody {
                    id: "a".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 2,
                        active_work_id: Some(1),
                        stack: vec![],
                        works: vec![],
                        entries: vec![],
                    },
                    outcome: WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Pop,
                        work_id: Some(1),
                        previous_work_id: Some(2),
                    },
                }))
            }
        }

        impl CollaborativeChildGoalService for PopClosingService {
            fn create_child_goal(
                &self,
                _meta: &mut ChildGoalMeta,
                _cwd: &Path,
                _parent_goal: &str,
                _handoff_reason: &str,
                _requested_command: &str,
                _human_request: &str,
            ) -> Result<(), CollaborativeChildGoalError> {
                Ok(())
            }

            fn close_child_goal(
                &self,
                meta: &ChildGoalMeta,
                cwd: &Path,
                _reason: ChildGoalCloseReason,
            ) -> Result<(), CollaborativeChildGoalError> {
                let Some(expected) = meta.work_id else {
                    return Ok(());
                };
                let context = MemoryContext {
                    cwd: Some(cwd.display().to_string()),
                    memory_space_id: None,
                };
                let active = match self
                    .client
                    .work_query("session", &context)
                    .map_err(|e| CollaborativeChildGoalError::Close(e.to_string()))?
                {
                    ClientResponse::WorkQueryResult(body) => body.snapshot.active_work_id,
                    ClientResponse::Error { message, .. } => {
                        return Err(CollaborativeChildGoalError::Close(message));
                    }
                    _ => {
                        return Err(CollaborativeChildGoalError::Close(
                            "unexpected work query response".into(),
                        ));
                    }
                };
                if active != Some(expected) {
                    return Err(CollaborativeChildGoalError::Close(format!(
                        "active work mismatch: active={active:?} expected={expected}"
                    )));
                }
                match self
                    .client
                    .work_apply("session", &context, WorkOperationDto::Pop)
                    .map_err(|e| CollaborativeChildGoalError::Close(e.to_string()))?
                {
                    ClientResponse::WorkApplyResult(body)
                        if body.outcome.kind == WorkMutationKindDto::Pop =>
                    {
                        Ok(())
                    }
                    ClientResponse::Error { message, .. } => {
                        Err(CollaborativeChildGoalError::Close(message))
                    }
                    _ => Err(CollaborativeChildGoalError::Close(
                        "unexpected work response".into(),
                    )),
                }
            }
        }

        let child_goal = ChildGoalMeta {
            id: "child".into(),
            handoff_id: "ho-test".into(),
            parent_goal_id: None,
            work_id: Some(2),
            auto_root_work_id: Some(1),
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let popped = Arc::new(Mutex::new(false));
        let service = PopClosingService {
            client: PopClient {
                popped: popped.clone(),
            },
        };
        close_child_goal_durable(
            &store,
            &service,
            &handoff_id,
            ChildGoalCloseReason::ControlReturned,
        )
        .unwrap();
        assert!(*popped.lock().unwrap());
        let checkpoint = store.load_checkpoint(&handoff_id).unwrap();
        assert_eq!(
            checkpoint.child_goal.close_state,
            Some(ChildGoalCloseState::Completed)
        );
    }

    #[test]
    fn close_durable_records_conflict_without_completed_state() {
        let child_goal = ChildGoalMeta {
            id: "child".into(),
            handoff_id: "ho-test".into(),
            parent_goal_id: None,
            work_id: Some(2),
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let error = close_child_goal_durable(
            &store,
            &FailingCloseService,
            &handoff_id,
            ChildGoalCloseReason::ControlReturned,
        )
        .expect_err("conflict");
        assert!(error.to_string().contains("active work mismatch"));
        let checkpoint = store.load_checkpoint(&handoff_id).unwrap();
        assert_eq!(
            checkpoint.child_goal.close_state,
            Some(ChildGoalCloseState::Conflict)
        );
        assert!(checkpoint.child_goal.close_reason.is_none());
    }

    #[test]
    fn handoff_requires_child_goal_service_when_work_open() {
        let child_goal = ChildGoalMeta {
            id: "child".into(),
            handoff_id: "ho-test".into(),
            parent_goal_id: None,
            work_id: Some(2),
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let checkpoint = store.load_checkpoint(&handoff_id).unwrap();
        assert!(handoff_requires_child_goal_service(&checkpoint));
    }

    #[test]
    fn handoff_requires_child_goal_service_false_when_completed() {
        let child_goal = ChildGoalMeta {
            id: "child".into(),
            handoff_id: "ho-test".into(),
            parent_goal_id: None,
            work_id: Some(2),
            auto_root_work_id: None,
            close_reason: Some(ChildGoalCloseReason::ControlReturned),
            close_state: Some(ChildGoalCloseState::Completed),
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let checkpoint = store.load_checkpoint(&handoff_id).unwrap();
        assert!(!handoff_requires_child_goal_service(&checkpoint));
    }
}
