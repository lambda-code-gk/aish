//! Work stack へ child goal を Push/Pop する adapter。

use std::path::Path;

use aibe_protocol::{
    ClientResponse, MemoryContext, WorkApplyResponseBody, WorkMutationKindDto, WorkOperationDto,
    WorkQueryResponseBody, WorkSnapshotDto,
};

use crate::domain::{ChildGoalCloseReason, ChildGoalMeta};
use crate::ports::outbound::{
    CollaborativeChildGoalError, CollaborativeChildGoalService, WorkClient,
};

pub struct AibeCollaborativeChildGoalService<C: WorkClient> {
    client: C,
    session_id: String,
    memory_space_id: String,
}

impl<C: WorkClient> AibeCollaborativeChildGoalService<C> {
    pub fn new(client: C, session_id: String, memory_space_id: String) -> Self {
        Self {
            client,
            session_id,
            memory_space_id,
        }
    }

    fn memory_context(&self, cwd: &Path) -> MemoryContext {
        MemoryContext {
            cwd: Some(cwd.display().to_string()),
            memory_space_id: Some(self.memory_space_id.clone()),
        }
    }

    fn query_snapshot(&self, cwd: &Path) -> Result<WorkSnapshotDto, CollaborativeChildGoalError> {
        let response = self
            .client
            .work_query(&self.session_id, &self.memory_context(cwd))
            .map_err(|e| CollaborativeChildGoalError::Create(e.to_string()))?;
        match response {
            ClientResponse::WorkQueryResult(WorkQueryResponseBody { snapshot, .. }) => Ok(snapshot),
            ClientResponse::Error { message, .. } => {
                Err(CollaborativeChildGoalError::Create(message))
            }
            _ => Err(CollaborativeChildGoalError::Create(
                "unexpected work query response".into(),
            )),
        }
    }

    fn ensure_active_work(
        &self,
        meta: &mut ChildGoalMeta,
        cwd: &Path,
    ) -> Result<(), CollaborativeChildGoalError> {
        let snapshot = self.query_snapshot(cwd)?;
        if snapshot.active_work_id.is_some() {
            return Ok(());
        }
        let goal = format!(
            "[collaborative handoff temporary root]\nHandoff ID: {}",
            meta.handoff_id
        );
        let response = self
            .client
            .work_apply(
                &self.session_id,
                &self.memory_context(cwd),
                WorkOperationDto::Start { goal },
            )
            .map_err(|e| CollaborativeChildGoalError::Create(e.to_string()))?;
        match response {
            ClientResponse::WorkApplyResult(body) => {
                if body.outcome.kind != WorkMutationKindDto::Start {
                    return Err(CollaborativeChildGoalError::Create(format!(
                        "unexpected work mutation: {:?}",
                        body.outcome.kind
                    )));
                }
                meta.auto_root_work_id = body.outcome.work_id;
                Ok(())
            }
            ClientResponse::Error { message, .. } => {
                Err(CollaborativeChildGoalError::Create(message))
            }
            _ => Err(CollaborativeChildGoalError::Create(
                "unexpected work response".into(),
            )),
        }
    }

    fn active_work_id(&self, cwd: &Path) -> Result<Option<u64>, CollaborativeChildGoalError> {
        Ok(self.query_snapshot(cwd)?.active_work_id)
    }

    fn pop_active_work(
        &self,
        cwd: &Path,
        expected_work_id: u64,
        reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError> {
        let active_work_id = self.active_work_id(cwd)?;
        match active_work_id {
            Some(active) if active == expected_work_id => {}
            Some(active) => {
                return Err(CollaborativeChildGoalError::Close(format!(
                    "active work mismatch: active={active} expected={expected_work_id}"
                )));
            }
            None => {
                return Err(CollaborativeChildGoalError::Close(
                    "no active work during child goal close".into(),
                ));
            }
        }
        let operation = match reason {
            ChildGoalCloseReason::ControlReturned | ChildGoalCloseReason::Compensated => {
                WorkOperationDto::Pop
            }
        };
        let response = self
            .client
            .work_apply(&self.session_id, &self.memory_context(cwd), operation)
            .map_err(|e| CollaborativeChildGoalError::Close(e.to_string()))?;
        match response {
            ClientResponse::WorkApplyResult(WorkApplyResponseBody { outcome, .. }) => {
                if outcome.kind != WorkMutationKindDto::Pop {
                    return Err(CollaborativeChildGoalError::Close(format!(
                        "unexpected work mutation: {:?} (reason: {reason:?})",
                        outcome.kind
                    )));
                }
                Ok(())
            }
            ClientResponse::Error { message, .. } => Err(CollaborativeChildGoalError::Close(
                format!("{message} (reason: {reason:?})"),
            )),
            _ => Err(CollaborativeChildGoalError::Close(
                "unexpected work response".into(),
            )),
        }
    }
}

impl<C: WorkClient> CollaborativeChildGoalService for AibeCollaborativeChildGoalService<C> {
    fn create_child_goal(
        &self,
        meta: &mut ChildGoalMeta,
        cwd: &Path,
        parent_goal: &str,
        handoff_reason: &str,
        requested_command: &str,
        human_request: &str,
    ) -> Result<(), CollaborativeChildGoalError> {
        self.ensure_active_work(meta, cwd)?;
        let parent_goal_ref = meta
            .parent_goal_id
            .as_deref()
            .map(|id| format!("parent goal entry: {id}"))
            .unwrap_or_else(|| format!("parent goal: {parent_goal}"));
        let goal = format!(
            "[collaborative child goal {id}]\n\
{parent_goal_ref}\n\
Handoff reason: {handoff_reason}\n\
Pending command: {requested_command}\n\
Human request: {human_request}\n\
Handoff ID: {handoff_id}",
            id = meta.id,
            handoff_id = meta.handoff_id,
        );
        let response = self
            .client
            .work_apply(
                &self.session_id,
                &self.memory_context(cwd),
                WorkOperationDto::Push { goal },
            )
            .map_err(|e| CollaborativeChildGoalError::Create(e.to_string()))?;
        match response {
            ClientResponse::WorkApplyResult(body) => {
                if body.outcome.kind != WorkMutationKindDto::Push {
                    return Err(CollaborativeChildGoalError::Create(format!(
                        "unexpected work mutation: {:?}",
                        body.outcome.kind
                    )));
                }
                meta.work_id = body.outcome.work_id;
                Ok(())
            }
            ClientResponse::Error { message, .. } => {
                Err(CollaborativeChildGoalError::Create(message))
            }
            _ => Err(CollaborativeChildGoalError::Create(
                "unexpected work response".into(),
            )),
        }
    }

    fn close_child_goal(
        &self,
        meta: &ChildGoalMeta,
        cwd: &Path,
        reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError> {
        if let Some(expected_work_id) = meta.work_id {
            return self.pop_active_work(cwd, expected_work_id, reason);
        }
        if let Some(root_id) = meta.auto_root_work_id {
            return self.pop_active_work(cwd, root_id, reason);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChildGoalAchievement;
    use aibe_protocol::{WorkApplyResponseBody, WorkMutationOutcomeDto};
    use std::sync::{Arc, Mutex};

    struct MockWorkClient {
        operations: Arc<Mutex<Vec<WorkOperationDto>>>,
        active_work_id: Arc<Mutex<Option<u64>>>,
    }

    impl WorkClient for MockWorkClient {
        fn work_query(
            &self,
            _session_id: &str,
            _context: &MemoryContext,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            Ok(ClientResponse::WorkQueryResult(
                aibe_protocol::WorkQueryResponseBody {
                    id: "test".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 1,
                        active_work_id: *self.active_work_id.lock().unwrap(),
                        stack: vec![],
                        works: vec![],
                        entries: vec![],
                    },
                },
            ))
        }

        fn work_apply(
            &self,
            _session_id: &str,
            context: &MemoryContext,
            operation: WorkOperationDto,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            assert!(context.cwd.is_some(), "cwd is required for work apply");
            self.operations.lock().unwrap().push(operation.clone());
            let outcome = match operation {
                WorkOperationDto::Start { .. } => {
                    *self.active_work_id.lock().unwrap() = Some(1);
                    WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Start,
                        work_id: Some(1),
                        previous_work_id: None,
                    }
                }
                WorkOperationDto::Push { .. } => {
                    *self.active_work_id.lock().unwrap() = Some(2);
                    WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Push,
                        work_id: Some(2),
                        previous_work_id: Some(1),
                    }
                }
                WorkOperationDto::Pop => {
                    let current = *self.active_work_id.lock().unwrap();
                    *self.active_work_id.lock().unwrap() =
                        if current == Some(2) { Some(1) } else { None };
                    WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Pop,
                        work_id: current,
                        previous_work_id: None,
                    }
                }
                _ => panic!("unexpected operation: {operation:?}"),
            };
            Ok(ClientResponse::WorkApplyResult(WorkApplyResponseBody {
                id: "test".into(),
                snapshot: WorkSnapshotDto {
                    revision: 1,
                    active_work_id: outcome.work_id,
                    stack: vec![],
                    works: vec![],
                    entries: vec![],
                },
                outcome,
            }))
        }
    }

    #[test]
    fn child_goal_starts_root_work_when_no_active_work() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(Mutex::new(None));
        let service = AibeCollaborativeChildGoalService::new(
            MockWorkClient {
                operations: operations.clone(),
                active_work_id: active.clone(),
            },
            "session".into(),
            "project_test".into(),
        );
        let mut meta = ChildGoalMeta {
            id: "child-1".into(),
            handoff_id: "ho-1".into(),
            parent_goal_id: Some("goal-parent".into()),
            work_id: None,
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: crate::domain::ChildGoalAchievement::Unknown,
        };
        service
            .create_child_goal(
                &mut meta,
                Path::new("/tmp/work"),
                "finish feature",
                "run tests",
                "cargo test",
                "please run tests",
            )
            .unwrap();
        assert_eq!(meta.auto_root_work_id, Some(1));
        assert_eq!(meta.work_id, Some(2));
        let ops = operations.lock().unwrap();
        assert!(matches!(ops[0], WorkOperationDto::Start { .. }));
        assert!(matches!(ops[1], WorkOperationDto::Push { .. }));
    }

    #[test]
    fn child_goal_uses_work_push_and_pop_with_cwd() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(Mutex::new(Some(1)));
        let service = AibeCollaborativeChildGoalService::new(
            MockWorkClient {
                operations: operations.clone(),
                active_work_id: active.clone(),
            },
            "session".into(),
            "project_test".into(),
        );
        let mut meta = ChildGoalMeta {
            id: "child-1".into(),
            handoff_id: "ho-1".into(),
            parent_goal_id: Some("goal-parent".into()),
            work_id: None,
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: crate::domain::ChildGoalAchievement::Unknown,
        };
        service
            .create_child_goal(
                &mut meta,
                Path::new("/tmp/work"),
                "finish feature",
                "run tests",
                "cargo test",
                "please run tests",
            )
            .unwrap();
        assert_eq!(meta.work_id, Some(2));
        service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::ControlReturned,
            )
            .unwrap();
        let ops = operations.lock().unwrap();
        assert!(matches!(ops[0], WorkOperationDto::Push { .. }));
        assert!(matches!(ops[1], WorkOperationDto::Pop));
    }

    #[test]
    fn child_goal_close_pops_auto_root_on_control_returned_when_push_never_succeeded() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(Mutex::new(Some(1)));
        let service = AibeCollaborativeChildGoalService::new(
            MockWorkClient {
                operations: operations.clone(),
                active_work_id: active.clone(),
            },
            "session".into(),
            "project_test".into(),
        );
        let meta = ChildGoalMeta {
            id: "child-1".into(),
            handoff_id: "ho-1".into(),
            parent_goal_id: None,
            work_id: None,
            auto_root_work_id: Some(1),
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::ControlReturned,
            )
            .unwrap();
        assert_eq!(*active.lock().unwrap(), None);
        let ops = operations.lock().unwrap();
        assert!(matches!(ops[0], WorkOperationDto::Pop));
    }

    #[test]
    fn child_goal_close_compensates_auto_root_when_push_never_succeeded() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(Mutex::new(Some(1)));
        let service = AibeCollaborativeChildGoalService::new(
            MockWorkClient {
                operations: operations.clone(),
                active_work_id: active.clone(),
            },
            "session".into(),
            "project_test".into(),
        );
        let meta = ChildGoalMeta {
            id: "child-1".into(),
            handoff_id: "ho-1".into(),
            parent_goal_id: None,
            work_id: None,
            auto_root_work_id: Some(1),
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        };
        service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::Compensated,
            )
            .unwrap();
        assert_eq!(*active.lock().unwrap(), None);
        let ops = operations.lock().unwrap();
        assert!(matches!(ops[0], WorkOperationDto::Pop));
    }

    #[test]
    fn child_goal_close_rejects_active_work_mismatch() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(Mutex::new(Some(99)));
        let service = AibeCollaborativeChildGoalService::new(
            MockWorkClient {
                operations,
                active_work_id: active,
            },
            "session".into(),
            "project_test".into(),
        );
        let meta = ChildGoalMeta {
            id: "child-1".into(),
            handoff_id: "ho-1".into(),
            parent_goal_id: None,
            work_id: Some(2),
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: crate::domain::ChildGoalAchievement::Unknown,
        };
        let error = service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::ControlReturned,
            )
            .expect_err("mismatch");
        assert!(error.to_string().contains("active work mismatch"));
    }
}
