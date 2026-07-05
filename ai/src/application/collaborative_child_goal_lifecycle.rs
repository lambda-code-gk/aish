//! child goal の Work close を checkpoint と同期して永続化する。

use std::path::Path;

use crate::domain::{
    child_goal_close_blocks_parent_resume, child_goal_close_is_conflict,
    child_goal_needs_work_close, mark_child_goal_closed, normalize_child_goal_meta,
    ChildGoalCloseReason, ChildGoalCloseState, HandoffCheckpoint,
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
    normalize_child_goal_meta(&mut checkpoint.child_goal);
    if !child_goal_needs_work_close(&checkpoint.child_goal) {
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
    checkpoint.child_goal.close_state = Some(ChildGoalCloseState::Closing);
    checkpoint.child_goal.close_error_message = None;
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
            checkpoint.child_goal.close_error_message = Some(message.clone());
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

/// 親 resume 前に child Work close を再試行する。Conflict/Failed のままなら Err。
pub fn reconcile_child_goal_before_parent_resume<S, C>(
    store: &S,
    child_goal_service: &C,
    handoff_id: &str,
) -> Result<(), CollaborativeChildGoalError>
where
    S: CheckpointRepository + HandoffRepository,
    C: CollaborativeChildGoalService + ?Sized,
{
    let mut checkpoint = store
        .load_checkpoint(handoff_id)
        .map_err(store_err_create)?;
    normalize_child_goal_meta(&mut checkpoint.child_goal);
    if child_goal_close_blocks_parent_resume(&checkpoint.child_goal)
        && child_goal_needs_work_close(&checkpoint.child_goal)
    {
        close_child_goal_durable(
            store,
            child_goal_service,
            handoff_id,
            ChildGoalCloseReason::ControlReturned,
        )?;
    }
    let checkpoint = store
        .load_checkpoint(handoff_id)
        .map_err(store_err_create)?;
    if child_goal_close_blocks_parent_resume(&checkpoint.child_goal) {
        let message = checkpoint
            .child_goal
            .close_error_message
            .clone()
            .unwrap_or_else(|| "child work close failed".into());
        return Err(CollaborativeChildGoalError::Close(message));
    }
    Ok(())
}

/// checkpoint 上で未完了の child Work がある場合、Noop ではなく実 service が必要。
pub fn handoff_requires_child_goal_service(checkpoint: &HandoffCheckpoint) -> bool {
    let mut child_goal = checkpoint.child_goal.clone();
    normalize_child_goal_meta(&mut child_goal);
    child_goal_needs_work_close(&child_goal)
}

pub fn child_goal_environment_patch(checkpoint: &HandoffCheckpoint) -> serde_json::Value {
    let mut metadata = serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = metadata.as_object_mut() {
        let mut child_goal = checkpoint.child_goal.clone();
        normalize_child_goal_meta(&mut child_goal);
        if let Some(mode) = child_goal.work_mode {
            object.insert(
                "child_work_mode".into(),
                serde_json::to_value(mode).unwrap(),
            );
        }
        if let Some(work_id) = child_goal.work_id {
            object.insert("child_work_id".into(), work_id.into());
        }
        if let Some(parent) = match child_goal.work_mode {
            Some(crate::domain::CollaborativeChildWorkMode::Pushed { parent_work_id, .. }) => {
                Some(parent_work_id)
            }
            _ => None,
        } {
            object.insert("parent_work_id".into(), parent.into());
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
        ChildGoalAchievement, ChildGoalMeta, CollaborativeChildWorkMode, HandoffCheckpoint,
        HandoffState, RequestedShellExec,
    };
    use crate::ports::outbound::{HandoffRepository, HandoffStoreError};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

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

        struct PopClient {
            popped: Arc<Mutex<bool>>,
        }

        impl WorkClient for PopClient {
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
                        stack: vec![1],
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
                        work_id: Some(2),
                        previous_work_id: Some(1),
                    },
                }))
            }
        }

        let child_goal = ChildGoalMeta {
            id: "child".into(),
            handoff_id: "ho-test".into(),
            parent_goal_id: None,
            work_id: Some(2),
            work_mode: Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            }),
            auto_root_work_id: None,
            close_reason: None,
            close_state: Some(ChildGoalCloseState::Open),
            close_error_message: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let popped = Arc::new(Mutex::new(false));
        let service = crate::adapters::outbound::AibeCollaborativeChildGoalService::new(
            PopClient {
                popped: popped.clone(),
            },
            "session".into(),
            "space".into(),
        );
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
            work_mode: Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            }),
            auto_root_work_id: None,
            close_reason: None,
            close_state: Some(ChildGoalCloseState::Open),
            close_error_message: None,
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
            work_mode: Some(CollaborativeChildWorkMode::StartedRoot { child_work_id: 2 }),
            auto_root_work_id: None,
            close_reason: None,
            close_state: Some(ChildGoalCloseState::Open),
            close_error_message: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let checkpoint = store.load_checkpoint(&handoff_id).unwrap();
        assert!(handoff_requires_child_goal_service(&checkpoint));
    }

    #[test]
    fn close_durable_retries_after_pop_without_second_pop() {
        use crate::ports::outbound::WorkClient;
        use aibe_protocol::{
            ClientResponse, MemoryContext, WorkApplyResponseBody, WorkItemDto, WorkMutationKindDto,
            WorkMutationOutcomeDto, WorkOperationDto, WorkQueryResponseBody, WorkSnapshotDto,
            WorkStatusDto,
        };

        struct PostPopCloseClient {
            pop_calls: Arc<Mutex<u32>>,
        }

        impl WorkClient for PostPopCloseClient {
            fn work_query(
                &self,
                _session_id: &str,
                _context: &MemoryContext,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                    id: "q".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 2,
                        active_work_id: Some(1),
                        stack: vec![],
                        works: vec![WorkItemDto {
                            id: 2,
                            title: "child".into(),
                            goal: "Handoff ID: ho-test".into(),
                            status: WorkStatusDto::Done,
                            parent_id: Some(1),
                            created_at_ms: 1,
                            updated_at_ms: 2,
                            finished_at_ms: Some(2),
                            focus: None,
                            summary: None,
                        }],
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
                if matches!(operation, WorkOperationDto::Pop) {
                    *self.pop_calls.lock().unwrap() += 1;
                }
                Err(crate::ports::outbound::AgentError::Request(
                    "should not pop when already done".into(),
                ))
            }
        }

        let child_goal = ChildGoalMeta {
            id: "child".into(),
            handoff_id: "ho-test".into(),
            parent_goal_id: None,
            work_id: Some(2),
            work_mode: Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            }),
            auto_root_work_id: None,
            close_reason: None,
            close_state: Some(ChildGoalCloseState::Closing),
            close_error_message: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let pop_calls = Arc::new(Mutex::new(0));
        let service = crate::adapters::outbound::AibeCollaborativeChildGoalService::new(
            PostPopCloseClient {
                pop_calls: pop_calls.clone(),
            },
            "session".into(),
            "space".into(),
        );
        close_child_goal_durable(
            &store,
            &service,
            &handoff_id,
            ChildGoalCloseReason::ControlReturned,
        )
        .unwrap();
        assert_eq!(*pop_calls.lock().unwrap(), 0);
        let checkpoint = store.load_checkpoint(&handoff_id).unwrap();
        assert_eq!(
            checkpoint.child_goal.close_state,
            Some(ChildGoalCloseState::Completed)
        );
    }

    #[test]
    fn close_durable_true_conflict_does_not_pop() {
        use crate::ports::outbound::WorkClient;
        use aibe_protocol::{
            ClientResponse, MemoryContext, WorkOperationDto, WorkQueryResponseBody, WorkSnapshotDto,
        };

        struct WrongActiveClient;

        impl crate::ports::outbound::WorkClient for WrongActiveClient {
            fn work_query(
                &self,
                _session_id: &str,
                _context: &MemoryContext,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                    id: "q".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 1,
                        active_work_id: Some(99),
                        stack: vec![1],
                        works: vec![],
                        entries: vec![],
                    },
                }))
            }

            fn work_apply(
                &self,
                _session_id: &str,
                _context: &MemoryContext,
                _operation: WorkOperationDto,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                panic!("must not apply work on conflict")
            }
        }

        let child_goal = ChildGoalMeta {
            id: "child".into(),
            handoff_id: "ho-test".into(),
            parent_goal_id: None,
            work_id: Some(2),
            work_mode: Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            }),
            auto_root_work_id: None,
            close_reason: None,
            close_state: Some(ChildGoalCloseState::Closing),
            close_error_message: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        let (store, handoff_id) = MemStore::with_checkpoint(child_goal);
        let service = crate::adapters::outbound::AibeCollaborativeChildGoalService::new(
            WrongActiveClient,
            "session".into(),
            "space".into(),
        );
        let error = close_child_goal_durable(
            &store,
            &service,
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
    }
}
