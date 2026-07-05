//! Work stack へ child goal を Push/Pop または Start/Finish する adapter。

use std::path::Path;

use aibe_protocol::{
    ClientResponse, MemoryContext, WorkApplyResponseBody, WorkItemDto, WorkMutationKindDto,
    WorkOperationDto, WorkQueryResponseBody, WorkSnapshotDto, WorkStatusDto,
};

use crate::domain::{
    normalize_child_goal_meta, ChildGoalCloseReason, ChildGoalCloseState, ChildGoalMeta,
    CollaborativeChildWorkMode,
};
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

    fn child_goal_text(
        meta: &ChildGoalMeta,
        parent_goal: &str,
        handoff_reason: &str,
        requested_command: &str,
        human_request: &str,
    ) -> String {
        let parent_goal_ref = meta
            .parent_goal_id
            .as_deref()
            .map(|id| format!("parent goal entry: {id}"))
            .unwrap_or_else(|| format!("parent goal: {parent_goal}"));
        format!(
            "[collaborative child goal {id}]\n\
{parent_goal_ref}\n\
Handoff reason: {handoff_reason}\n\
Pending command: {requested_command}\n\
Human request: {human_request}\n\
Handoff ID: {handoff_id}",
            id = meta.id,
            handoff_id = meta.handoff_id,
        )
    }

    fn apply_operation(
        &self,
        cwd: &Path,
        operation: WorkOperationDto,
        phase: &str,
    ) -> Result<WorkApplyResponseBody, CollaborativeChildGoalError> {
        let response = self
            .client
            .work_apply(&self.session_id, &self.memory_context(cwd), operation)
            .map_err(|e| CollaborativeChildGoalError::Close(e.to_string()))?;
        match response {
            ClientResponse::WorkApplyResult(body) => Ok(body),
            ClientResponse::Error { message, .. } => Err(CollaborativeChildGoalError::Close(
                format!("{message} ({phase})"),
            )),
            _ => Err(CollaborativeChildGoalError::Close(format!(
                "unexpected work response ({phase})"
            ))),
        }
    }

    fn work_status(snapshot: &WorkSnapshotDto, work_id: u64) -> Option<WorkStatusDto> {
        snapshot
            .works
            .iter()
            .find(|work| work.id == work_id)
            .map(|work| work.status)
    }

    fn pop_already_done(
        snapshot: &WorkSnapshotDto,
        parent_work_id: u64,
        child_work_id: u64,
    ) -> bool {
        snapshot.active_work_id == Some(parent_work_id)
            && Self::work_status(snapshot, child_work_id) == Some(WorkStatusDto::Done)
            && !snapshot.stack.contains(&child_work_id)
    }

    fn finish_already_done(snapshot: &WorkSnapshotDto, child_work_id: u64) -> bool {
        snapshot.active_work_id.is_none()
            && Self::work_status(snapshot, child_work_id) == Some(WorkStatusDto::Done)
    }

    fn handoff_id_marker(handoff_id: &str) -> String {
        format!("Handoff ID: {handoff_id}")
    }

    fn works_matching_handoff<'a>(
        snapshot: &'a WorkSnapshotDto,
        handoff_id: &str,
    ) -> Vec<&'a WorkItemDto> {
        let marker = Self::handoff_id_marker(handoff_id);
        snapshot
            .works
            .iter()
            .filter(|work| work.goal.contains(&marker))
            .collect()
    }

    fn reconcile_created_work(
        meta: &ChildGoalMeta,
        snapshot: &WorkSnapshotDto,
        parent_before_push: Option<u64>,
    ) -> Result<Option<CollaborativeChildWorkMode>, CollaborativeChildGoalError> {
        let matches = Self::works_matching_handoff(snapshot, &meta.handoff_id);
        match matches.len() {
            0 => Ok(None),
            1 => {
                let work = matches[0];
                let child_work_id = work.id;
                let is_active = snapshot.active_work_id == Some(child_work_id);
                let on_stack = snapshot.stack.contains(&child_work_id);
                if let Some(parent_work_id) = parent_before_push {
                    if is_active && snapshot.stack.last().copied() == Some(parent_work_id) {
                        return Ok(Some(CollaborativeChildWorkMode::Pushed {
                            parent_work_id,
                            child_work_id,
                        }));
                    }
                    if Self::pop_already_done(snapshot, parent_work_id, child_work_id) {
                        return Ok(Some(CollaborativeChildWorkMode::Pushed {
                            parent_work_id,
                            child_work_id,
                        }));
                    }
                } else if (is_active && snapshot.stack.is_empty())
                    || Self::finish_already_done(snapshot, child_work_id)
                {
                    return Ok(Some(CollaborativeChildWorkMode::StartedRoot {
                        child_work_id,
                    }));
                }
                if is_active || on_stack {
                    return Err(CollaborativeChildGoalError::Create(
                        "initialization conflict: manual reconciliation required".into(),
                    ));
                }
                Err(CollaborativeChildGoalError::Create(
                    "initialization conflict: matched work is not active or on stack".into(),
                ))
            }
            _ => Err(CollaborativeChildGoalError::Create(
                "initialization conflict: multiple works match handoff id".into(),
            )),
        }
    }

    fn apply_create_operation(
        &self,
        meta: &mut ChildGoalMeta,
        cwd: &Path,
        snapshot: &WorkSnapshotDto,
        goal: String,
    ) -> Result<(), CollaborativeChildGoalError> {
        let parent_before_push = snapshot.active_work_id;
        let operation = if parent_before_push.is_some() {
            WorkOperationDto::Push { goal }
        } else {
            WorkOperationDto::Start { goal }
        };
        let apply_result =
            self.client
                .work_apply(&self.session_id, &self.memory_context(cwd), operation);
        let body = match apply_result {
            Ok(ClientResponse::WorkApplyResult(body)) => body,
            Ok(ClientResponse::Error { message, .. }) => {
                return Err(CollaborativeChildGoalError::Create(message));
            }
            Ok(_) => {
                return Err(CollaborativeChildGoalError::Create(
                    "unexpected work response".into(),
                ));
            }
            Err(error) => {
                let snapshot_after = self.query_snapshot(cwd)?;
                if let Some(mode) =
                    Self::reconcile_created_work(meta, &snapshot_after, parent_before_push)?
                {
                    meta.work_mode = Some(mode);
                    meta.work_id = meta.work_mode.map(|mode| mode.child_work_id());
                    meta.auto_root_work_id = None;
                    meta.close_state = Some(ChildGoalCloseState::Open);
                    meta.close_error_message = None;
                    return Ok(());
                }
                return Err(CollaborativeChildGoalError::Create(error.to_string()));
            }
        };
        let expected_kind = if parent_before_push.is_some() {
            WorkMutationKindDto::Push
        } else {
            WorkMutationKindDto::Start
        };
        if body.outcome.kind != expected_kind {
            return Err(CollaborativeChildGoalError::Create(format!(
                "unexpected work mutation: {:?}",
                body.outcome.kind
            )));
        }
        let child_work_id = body.outcome.work_id.ok_or_else(|| {
            CollaborativeChildGoalError::Create("work create missing work id".into())
        })?;
        meta.work_mode = Some(if let Some(parent_work_id) = parent_before_push {
            CollaborativeChildWorkMode::Pushed {
                parent_work_id,
                child_work_id,
            }
        } else {
            CollaborativeChildWorkMode::StartedRoot { child_work_id }
        });
        meta.work_id = meta.work_mode.map(|mode| mode.child_work_id());
        meta.auto_root_work_id = None;
        meta.close_state = Some(ChildGoalCloseState::Open);
        meta.close_error_message = None;
        Ok(())
    }

    fn pop_child(
        &self,
        cwd: &Path,
        snapshot: &WorkSnapshotDto,
        parent_work_id: u64,
        child_work_id: u64,
        reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError> {
        match snapshot.active_work_id {
            Some(active) if active == child_work_id => {
                let Some(stack_parent) = snapshot.stack.last().copied() else {
                    return Err(CollaborativeChildGoalError::Close(
                        "active work conflict: child work expected on stack".into(),
                    ));
                };
                if stack_parent != parent_work_id {
                    return Err(CollaborativeChildGoalError::Close(format!(
                        "active work conflict: stack parent={stack_parent} expected={parent_work_id}"
                    )));
                }
            }
            Some(active) if active == parent_work_id => {
                if Self::pop_already_done(snapshot, parent_work_id, child_work_id) {
                    return Ok(());
                }
                return Err(CollaborativeChildGoalError::Close(format!(
                    "active work conflict: active={active} expected child={child_work_id}"
                )));
            }
            Some(active) => {
                return Err(CollaborativeChildGoalError::Close(format!(
                    "active work mismatch: active={active} expected child={child_work_id}"
                )));
            }
            None => {
                if Self::pop_already_done(snapshot, parent_work_id, child_work_id) {
                    return Ok(());
                }
                return Err(CollaborativeChildGoalError::Close(
                    "no active work during child goal close".into(),
                ));
            }
        }
        let body = self.apply_operation(cwd, WorkOperationDto::Pop, "pop child")?;
        if body.outcome.kind != WorkMutationKindDto::Pop {
            return Err(CollaborativeChildGoalError::Close(format!(
                "unexpected work mutation: {:?} (reason: {reason:?})",
                body.outcome.kind
            )));
        }
        Ok(())
    }

    fn finish_root_child(
        &self,
        cwd: &Path,
        snapshot: &WorkSnapshotDto,
        child_work_id: u64,
        reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError> {
        match snapshot.active_work_id {
            Some(active) if active == child_work_id => {
                if !snapshot.stack.is_empty() {
                    return Err(CollaborativeChildGoalError::Close(
                        "active work conflict: root child work must have empty stack".into(),
                    ));
                }
            }
            Some(active) => {
                return Err(CollaborativeChildGoalError::Close(format!(
                    "active work mismatch: active={active} expected child={child_work_id}"
                )));
            }
            None => {
                if Self::finish_already_done(snapshot, child_work_id) {
                    return Ok(());
                }
                return Err(CollaborativeChildGoalError::Close(
                    "no active work during child goal close".into(),
                ));
            }
        }
        let body = self.apply_operation(cwd, WorkOperationDto::Finish, "finish root child")?;
        if body.outcome.kind != WorkMutationKindDto::Finish {
            return Err(CollaborativeChildGoalError::Close(format!(
                "unexpected work mutation: {:?} (reason: {reason:?})",
                body.outcome.kind
            )));
        }
        Ok(())
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
        let snapshot = self.query_snapshot(cwd)?;
        let goal = Self::child_goal_text(
            meta,
            parent_goal,
            handoff_reason,
            requested_command,
            human_request,
        );
        self.apply_create_operation(meta, cwd, &snapshot, goal)
    }

    fn close_child_goal(
        &self,
        meta: &ChildGoalMeta,
        cwd: &Path,
        reason: ChildGoalCloseReason,
    ) -> Result<(), CollaborativeChildGoalError> {
        let mut normalized = meta.clone();
        normalize_child_goal_meta(&mut normalized);
        let Some(mode) = normalized.work_mode else {
            return Ok(());
        };
        let snapshot = self.query_snapshot(cwd)?;
        match mode {
            CollaborativeChildWorkMode::Pushed {
                parent_work_id,
                child_work_id,
            } => self.pop_child(cwd, &snapshot, parent_work_id, child_work_id, reason),
            CollaborativeChildWorkMode::StartedRoot { child_work_id } => {
                self.finish_root_child(cwd, &snapshot, child_work_id, reason)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChildGoalAchievement;
    use aibe_protocol::{
        WorkApplyResponseBody, WorkItemDto, WorkMutationOutcomeDto, WorkStatusDto,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Default, Clone)]
    struct MockWorkState {
        active_work_id: Option<u64>,
        stack: Vec<u64>,
        next_work_id: u64,
    }

    struct DomainMockWorkClient {
        state: Arc<Mutex<MockWorkState>>,
        operations: Arc<Mutex<Vec<WorkOperationDto>>>,
    }

    impl DomainMockWorkClient {
        fn snapshot(&self) -> WorkSnapshotDto {
            let state = self.state.lock().unwrap();
            Self::snapshot_locked(&state)
        }

        fn snapshot_locked(state: &MockWorkState) -> WorkSnapshotDto {
            WorkSnapshotDto {
                revision: 1,
                active_work_id: state.active_work_id,
                stack: state.stack.clone(),
                works: vec![],
                entries: vec![],
            }
        }
    }

    impl WorkClient for DomainMockWorkClient {
        fn work_query(
            &self,
            _session_id: &str,
            _context: &MemoryContext,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                id: "test".into(),
                snapshot: self.snapshot(),
            }))
        }

        fn work_apply(
            &self,
            _session_id: &str,
            context: &MemoryContext,
            operation: WorkOperationDto,
        ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
            assert!(context.cwd.is_some(), "cwd is required for work apply");
            self.operations.lock().unwrap().push(operation.clone());
            let mut state = self.state.lock().unwrap();
            let outcome = match operation {
                WorkOperationDto::Start { .. } => {
                    if !state.stack.is_empty() {
                        return Err(crate::ports::outbound::AgentError::Request(
                            "work stack is not empty".into(),
                        ));
                    }
                    let work_id = state.next_work_id.max(1);
                    state.next_work_id = work_id.saturating_add(1);
                    state.active_work_id = Some(work_id);
                    WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Start,
                        work_id: Some(work_id),
                        previous_work_id: None,
                    }
                }
                WorkOperationDto::Push { .. } => {
                    let parent = state.active_work_id.ok_or_else(|| {
                        crate::ports::outbound::AgentError::Request("no active work".into())
                    })?;
                    let work_id = state.next_work_id.max(1);
                    state.next_work_id = work_id.saturating_add(1);
                    state.stack.push(parent);
                    state.active_work_id = Some(work_id);
                    WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Push,
                        work_id: Some(work_id),
                        previous_work_id: Some(parent),
                    }
                }
                WorkOperationDto::Pop => {
                    let active = state.active_work_id.ok_or_else(|| {
                        crate::ports::outbound::AgentError::Request("no active work".into())
                    })?;
                    let parent = state.stack.pop().ok_or_else(|| {
                        crate::ports::outbound::AgentError::Request("empty stack".into())
                    })?;
                    state.active_work_id = Some(parent);
                    WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Pop,
                        work_id: Some(active),
                        previous_work_id: Some(parent),
                    }
                }
                WorkOperationDto::Finish => {
                    if !state.stack.is_empty() {
                        return Err(crate::ports::outbound::AgentError::Request(
                            "work stack is not empty".into(),
                        ));
                    }
                    let active = state.active_work_id.ok_or_else(|| {
                        crate::ports::outbound::AgentError::Request("no active work".into())
                    })?;
                    state.active_work_id = None;
                    WorkMutationOutcomeDto {
                        kind: WorkMutationKindDto::Finish,
                        work_id: Some(active),
                        previous_work_id: None,
                    }
                }
                _ => {
                    return Err(crate::ports::outbound::AgentError::Request(
                        "unsupported operation".into(),
                    ));
                }
            };
            Ok(ClientResponse::WorkApplyResult(WorkApplyResponseBody {
                id: "test".into(),
                snapshot: Self::snapshot_locked(&state),
                outcome,
            }))
        }
    }

    fn service_with_active(
        active: Option<u64>,
        stack: Vec<u64>,
    ) -> AibeCollaborativeChildGoalService<DomainMockWorkClient> {
        let next = active
            .unwrap_or(0)
            .max(stack.iter().copied().max().unwrap_or(0))
            + 1;
        AibeCollaborativeChildGoalService::new(
            DomainMockWorkClient {
                state: Arc::new(Mutex::new(MockWorkState {
                    active_work_id: active,
                    stack,
                    next_work_id: next,
                })),
                operations: Arc::new(Mutex::new(Vec::new())),
            },
            "session".into(),
            "project_test".into(),
        )
    }

    fn sample_meta() -> ChildGoalMeta {
        ChildGoalMeta {
            id: "child-1".into(),
            handoff_id: "ho-1".into(),
            parent_goal_id: Some("goal-parent".into()),
            work_id: None,
            work_mode: None,
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            close_error_message: None,
            achievement: ChildGoalAchievement::Unknown,
        }
    }

    #[test]
    fn child_goal_starts_root_work_when_no_active_work() {
        let client = DomainMockWorkClient {
            state: Arc::new(Mutex::new(MockWorkState::default())),
            operations: Arc::new(Mutex::new(Vec::new())),
        };
        let operations = client.operations.clone();
        let service =
            AibeCollaborativeChildGoalService::new(client, "session".into(), "project_test".into());
        let mut meta = sample_meta();
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
        assert_eq!(
            meta.work_mode,
            Some(CollaborativeChildWorkMode::StartedRoot { child_work_id: 1 })
        );
        assert_eq!(meta.work_id, Some(1));
        assert_eq!(meta.close_state, Some(ChildGoalCloseState::Open));
        let ops = operations.lock().unwrap();
        assert!(matches!(ops[0], WorkOperationDto::Start { .. }));
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn child_goal_uses_work_push_and_pop_with_cwd() {
        let client = DomainMockWorkClient {
            state: Arc::new(Mutex::new(MockWorkState {
                active_work_id: Some(1),
                stack: vec![],
                next_work_id: 2,
            })),
            operations: Arc::new(Mutex::new(Vec::new())),
        };
        let operations = client.operations.clone();
        let service =
            AibeCollaborativeChildGoalService::new(client, "session".into(), "project_test".into());
        let mut meta = sample_meta();
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
        assert_eq!(
            meta.work_mode,
            Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            })
        );
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
    fn child_goal_finish_root_on_control_returned() {
        let service = service_with_active(None, vec![]);
        let mut meta = sample_meta();
        service
            .create_child_goal(
                &mut meta,
                Path::new("/tmp/work"),
                "goal",
                "reason",
                "cmd",
                "human",
            )
            .unwrap();
        service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::ControlReturned,
            )
            .unwrap();
        assert_eq!(
            service
                .query_snapshot(Path::new("/tmp/work"))
                .unwrap()
                .active_work_id,
            None
        );
    }

    #[test]
    fn child_goal_push_fails_without_active_work() {
        let client = DomainMockWorkClient {
            state: Arc::new(Mutex::new(MockWorkState::default())),
            operations: Arc::new(Mutex::new(Vec::new())),
        };
        let service =
            AibeCollaborativeChildGoalService::new(client, "session".into(), "project_test".into());
        let mut meta = sample_meta();
        // force push path by setting active then clearing state incorrectly isn't possible;
        // Start path is taken when no active work — verify Pop on empty stack fails.
        service
            .create_child_goal(
                &mut meta,
                Path::new("/tmp/work"),
                "goal",
                "reason",
                "cmd",
                "human",
            )
            .unwrap();
        let broken = ChildGoalMeta {
            work_mode: Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 99,
                child_work_id: meta.work_id.unwrap(),
            }),
            ..meta
        };
        let error = service
            .close_child_goal(
                &broken,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::ControlReturned,
            )
            .expect_err("stack conflict");
        assert!(error.to_string().contains("active work conflict"));
    }

    #[test]
    fn child_goal_close_rejects_active_work_mismatch() {
        let service = service_with_active(Some(99), vec![1]);
        let meta = ChildGoalMeta {
            work_mode: Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            }),
            work_id: Some(2),
            ..sample_meta()
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

    #[test]
    fn pop_already_done_skips_second_pop() {
        let meta = ChildGoalMeta {
            work_mode: Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            }),
            work_id: Some(2),
            ..sample_meta()
        };
        let client = DomainMockWorkClient {
            state: Arc::new(Mutex::new(MockWorkState {
                active_work_id: Some(1),
                stack: vec![],
                next_work_id: 3,
            })),
            operations: Arc::new(Mutex::new(Vec::new())),
        };
        struct PostPopClient {
            inner: DomainMockWorkClient,
        }
        impl WorkClient for PostPopClient {
            fn work_query(
                &self,
                session_id: &str,
                context: &MemoryContext,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                let _ = self.inner.work_query(session_id, context)?;
                Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                    id: "test".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 2,
                        active_work_id: Some(1),
                        stack: vec![],
                        works: vec![WorkItemDto {
                            id: 2,
                            title: "child".into(),
                            goal: format!("Handoff ID: {}", sample_meta().handoff_id),
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
                session_id: &str,
                context: &MemoryContext,
                operation: WorkOperationDto,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                self.inner.work_apply(session_id, context, operation)
            }
        }
        let service = AibeCollaborativeChildGoalService::new(
            PostPopClient { inner: client },
            "session".into(),
            "project_test".into(),
        );
        service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::ControlReturned,
            )
            .unwrap();
        assert_eq!(
            service
                .query_snapshot(Path::new("/tmp/work"))
                .unwrap()
                .active_work_id,
            Some(1)
        );
    }

    #[test]
    fn finish_already_done_skips_second_finish() {
        struct PostFinishClient;
        impl WorkClient for PostFinishClient {
            fn work_query(
                &self,
                _session_id: &str,
                _context: &MemoryContext,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                    id: "test".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 2,
                        active_work_id: None,
                        stack: vec![],
                        works: vec![WorkItemDto {
                            id: 1,
                            title: "child".into(),
                            goal: format!("Handoff ID: {}", sample_meta().handoff_id),
                            status: WorkStatusDto::Done,
                            parent_id: None,
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
                _operation: WorkOperationDto,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                panic!("Finish must not be called when already done")
            }
        }
        let service = AibeCollaborativeChildGoalService::new(
            PostFinishClient,
            "session".into(),
            "space".into(),
        );
        let meta = ChildGoalMeta {
            work_mode: Some(CollaborativeChildWorkMode::StartedRoot { child_work_id: 1 }),
            work_id: Some(1),
            ..sample_meta()
        };
        service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::ControlReturned,
            )
            .unwrap();
    }

    #[test]
    fn create_recovers_after_push_response_loss() {
        struct PushResponseLossClient {
            state: Arc<Mutex<MockWorkState>>,
            goal: Arc<Mutex<Option<String>>>,
            fail_apply_once: Arc<Mutex<bool>>,
        }
        impl WorkClient for PushResponseLossClient {
            fn work_query(
                &self,
                _session_id: &str,
                _context: &MemoryContext,
            ) -> Result<ClientResponse, crate::ports::outbound::AgentError> {
                let state = self.state.lock().unwrap();
                let works = self
                    .goal
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|goal| {
                        vec![WorkItemDto {
                            id: 2,
                            title: goal.clone(),
                            goal: goal.clone(),
                            status: WorkStatusDto::Active,
                            parent_id: Some(1),
                            created_at_ms: 1,
                            updated_at_ms: 1,
                            finished_at_ms: None,
                            focus: None,
                            summary: None,
                        }]
                    })
                    .unwrap_or_default();
                Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                    id: "test".into(),
                    snapshot: WorkSnapshotDto {
                        revision: 1,
                        active_work_id: state.active_work_id,
                        stack: state.stack.clone(),
                        works,
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
                if let WorkOperationDto::Push { goal } = &operation {
                    if *self.fail_apply_once.lock().unwrap() {
                        *self.fail_apply_once.lock().unwrap() = false;
                        let mut state = self.state.lock().unwrap();
                        state.stack.push(1);
                        state.active_work_id = Some(2);
                        *self.goal.lock().unwrap() = Some(goal.clone());
                        return Err(crate::ports::outbound::AgentError::Request(
                            "response lost".into(),
                        ));
                    }
                }
                Err(crate::ports::outbound::AgentError::Request(
                    "unexpected apply".into(),
                ))
            }
        }
        let service = AibeCollaborativeChildGoalService::new(
            PushResponseLossClient {
                state: Arc::new(Mutex::new(MockWorkState {
                    active_work_id: Some(1),
                    stack: vec![],
                    next_work_id: 2,
                })),
                goal: Arc::new(Mutex::new(None)),
                fail_apply_once: Arc::new(Mutex::new(true)),
            },
            "session".into(),
            "space".into(),
        );
        let mut meta = sample_meta();
        service
            .create_child_goal(
                &mut meta,
                Path::new("/tmp/work"),
                "goal",
                "reason",
                "cmd",
                "human",
            )
            .unwrap();
        assert_eq!(
            meta.work_mode,
            Some(CollaborativeChildWorkMode::Pushed {
                parent_work_id: 1,
                child_work_id: 2,
            })
        );
    }

    #[test]
    fn root_pop_is_rejected() {
        let service = service_with_active(Some(1), vec![]);
        let meta = ChildGoalMeta {
            work_mode: Some(CollaborativeChildWorkMode::StartedRoot { child_work_id: 1 }),
            work_id: Some(1),
            ..sample_meta()
        };
        service
            .close_child_goal(
                &meta,
                Path::new("/tmp/work"),
                ChildGoalCloseReason::Compensated,
            )
            .unwrap();
        assert_eq!(
            service
                .query_snapshot(Path::new("/tmp/work"))
                .unwrap()
                .active_work_id,
            None
        );
    }
}
