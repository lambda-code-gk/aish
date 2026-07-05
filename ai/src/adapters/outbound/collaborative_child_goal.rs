//! Work stack へ child goal を Push/Pop する adapter。

use std::path::Path;

use aibe_protocol::{ClientResponse, MemoryContext, WorkMutationKindDto, WorkOperationDto};

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
        if meta.work_id.is_none() {
            return Ok(());
        }
        let operation = match reason {
            ChildGoalCloseReason::ControlReturned => WorkOperationDto::Pop,
        };
        let response = self
            .client
            .work_apply(&self.session_id, &self.memory_context(cwd), operation)
            .map_err(|e| CollaborativeChildGoalError::Close(e.to_string()))?;
        match response {
            ClientResponse::WorkApplyResult(body) => {
                if body.outcome.kind != WorkMutationKindDto::Pop {
                    return Err(CollaborativeChildGoalError::Close(format!(
                        "unexpected work mutation: {:?} (reason: {reason:?})",
                        body.outcome.kind
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

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::{WorkApplyResponseBody, WorkMutationOutcomeDto, WorkSnapshotDto};
    use std::sync::{Arc, Mutex};

    struct MockWorkClient {
        operations: Arc<Mutex<Vec<WorkOperationDto>>>,
    }

    impl WorkClient for MockWorkClient {
        fn work_query(
            &self,
            _session_id: &str,
            _context: &MemoryContext,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            unimplemented!()
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
                WorkOperationDto::Push { .. } => WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Push,
                    work_id: Some(2),
                    previous_work_id: Some(1),
                },
                WorkOperationDto::Pop => WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Pop,
                    work_id: Some(1),
                    previous_work_id: Some(2),
                },
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
    fn child_goal_uses_work_push_and_pop_with_cwd() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let service = AibeCollaborativeChildGoalService::new(
            MockWorkClient {
                operations: operations.clone(),
            },
            "session".into(),
            "project_test".into(),
        );
        let mut meta = ChildGoalMeta {
            id: "child-1".into(),
            handoff_id: "ho-1".into(),
            parent_goal_id: Some("goal-parent".into()),
            work_id: None,
            close_reason: None,
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
}
