//! aibe Work domain 制約に沿った mock WorkClient（0055 統合テスト用）。

use std::sync::{Arc, Mutex};

use aibe_protocol::{
    ClientResponse, MemoryContext, WorkApplyResponseBody, WorkMutationKindDto,
    WorkMutationOutcomeDto, WorkOperationDto, WorkQueryResponseBody, WorkSnapshotDto,
};

#[derive(Default, Clone)]
pub struct MockWorkState {
    pub active_work_id: Option<u64>,
    pub stack: Vec<u64>,
    pub next_work_id: u64,
}

pub struct DomainMockWorkClient {
    pub state: Arc<Mutex<MockWorkState>>,
}

impl DomainMockWorkClient {
    pub fn new(state: MockWorkState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn with_active(active: Option<u64>) -> Self {
        let next = active.unwrap_or(0) + 1;
        Self::new(MockWorkState {
            active_work_id: active,
            stack: vec![],
            next_work_id: next,
        })
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

impl ai::ports::outbound::WorkClient for DomainMockWorkClient {
    fn work_query(
        &self,
        _session_id: &str,
        _context: &MemoryContext,
    ) -> Result<ClientResponse, ai::ports::outbound::AgentError> {
        Ok(ClientResponse::WorkQueryResult(WorkQueryResponseBody {
            id: "q".into(),
            snapshot: Self::snapshot_locked(&self.state.lock().unwrap()),
        }))
    }

    fn work_apply(
        &self,
        _session_id: &str,
        context: &MemoryContext,
        operation: WorkOperationDto,
    ) -> Result<ClientResponse, ai::ports::outbound::AgentError> {
        assert!(context.cwd.is_some(), "cwd is required for work apply");
        let mut state = self.state.lock().unwrap();
        let outcome = match operation {
            WorkOperationDto::Start { .. } => {
                if !state.stack.is_empty() {
                    return Err(ai::ports::outbound::AgentError::Request(
                        "work stack is not empty".into(),
                    ));
                }
                let work_id = state.next_work_id;
                state.next_work_id += 1;
                state.active_work_id = Some(work_id);
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Start,
                    work_id: Some(work_id),
                    previous_work_id: None,
                }
            }
            WorkOperationDto::Push { .. } => {
                let parent = state.active_work_id.ok_or_else(|| {
                    ai::ports::outbound::AgentError::Request("no active work".into())
                })?;
                let work_id = state.next_work_id;
                state.next_work_id += 1;
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
                    ai::ports::outbound::AgentError::Request("no active work".into())
                })?;
                let parent = state.stack.pop().ok_or_else(|| {
                    ai::ports::outbound::AgentError::Request("empty stack".into())
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
                    return Err(ai::ports::outbound::AgentError::Request(
                        "work stack is not empty".into(),
                    ));
                }
                let active = state.active_work_id.ok_or_else(|| {
                    ai::ports::outbound::AgentError::Request("no active work".into())
                })?;
                state.active_work_id = None;
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Finish,
                    work_id: Some(active),
                    previous_work_id: None,
                }
            }
            _ => {
                return Err(ai::ports::outbound::AgentError::Request(
                    "unsupported operation".into(),
                ));
            }
        };
        Ok(ClientResponse::WorkApplyResult(WorkApplyResponseBody {
            id: "a".into(),
            snapshot: Self::snapshot_locked(&state),
            outcome,
        }))
    }
}

/// 操作名を記録する DomainMockWorkClient ラッパー。
pub struct TrackingDomainMockWorkClient {
    pub inner: DomainMockWorkClient,
    pub ops: Arc<Mutex<Vec<String>>>,
}

impl TrackingDomainMockWorkClient {
    pub fn with_active(active: Option<u64>) -> Self {
        Self {
            inner: DomainMockWorkClient::with_active(active),
            ops: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn active_work_id(&self) -> Option<u64> {
        self.inner.state.lock().unwrap().active_work_id
    }
}

impl ai::ports::outbound::WorkClient for TrackingDomainMockWorkClient {
    fn work_query(
        &self,
        session_id: &str,
        context: &MemoryContext,
    ) -> Result<ClientResponse, ai::ports::outbound::AgentError> {
        self.inner.work_query(session_id, context)
    }

    fn work_apply(
        &self,
        session_id: &str,
        context: &MemoryContext,
        operation: WorkOperationDto,
    ) -> Result<ClientResponse, ai::ports::outbound::AgentError> {
        let label = match &operation {
            WorkOperationDto::Start { .. } => "Start",
            WorkOperationDto::Push { .. } => "Push",
            WorkOperationDto::Pop => "Pop",
            WorkOperationDto::Finish => "Finish",
            _ => "Other",
        };
        self.ops.lock().unwrap().push(label.into());
        self.inner.work_apply(session_id, context, operation)
    }
}
