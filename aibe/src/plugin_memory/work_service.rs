//! Work RPC application service。

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aibe_protocol::{
    is_valid_session_id, ClientResponse, ErrorCode, MemoryContext, WorkApplyResponseBody,
    WorkOperationDto, WorkQueryResponseBody,
};

use crate::domain::{Capability, WorkMutationError};
use crate::ports::outbound::{
    CapabilityPolicy, MemorySpaceResolver, WorkStore, WorkStoreContext, WorkStoreError,
};

pub struct WorkService {
    store: Arc<dyn WorkStore>,
    resolver: Arc<dyn MemorySpaceResolver>,
    capability_policy: Arc<dyn CapabilityPolicy>,
}

impl WorkService {
    pub fn new(
        store: Arc<dyn WorkStore>,
        resolver: Arc<dyn MemorySpaceResolver>,
        capability_policy: Arc<dyn CapabilityPolicy>,
    ) -> Self {
        Self {
            store,
            resolver,
            capability_policy,
        }
    }

    pub fn query(&self, id: String, session_id: String, context: &MemoryContext) -> ClientResponse {
        if !is_valid_session_id(&session_id) {
            return invalid(id, "invalid session_id");
        }
        if let Err(denied) = self.capability_policy.require(Capability::MemoryRead) {
            return invalid(id, &denied.message());
        }
        let store_ctx = match self.resolve_context(&session_id, context) {
            Ok(ctx) => ctx,
            Err(()) => return invalid(id, "invalid work context"),
        };
        match self.store.load(&store_ctx) {
            Ok(state) => ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                id,
                snapshot: state.to_snapshot_dto(),
            }),
            Err(error) => map_store_error(id, error),
        }
    }

    pub fn apply(
        &self,
        id: String,
        session_id: String,
        context: &MemoryContext,
        operation: WorkOperationDto,
    ) -> ClientResponse {
        if !is_valid_session_id(&session_id) {
            return invalid(id, "invalid session_id");
        }
        if let Err(denied) = self.capability_policy.require(Capability::MemoryWrite) {
            return invalid(id, &denied.message());
        }
        if operation.validate().is_err() {
            return invalid(id, "invalid work operation");
        }
        let store_ctx = match self.resolve_context(&session_id, context) {
            Ok(ctx) => ctx,
            Err(()) => return invalid(id, "invalid work context"),
        };
        let now_ms = current_time_ms();
        let mut outcome = None;
        let result = self.store.mutate(&store_ctx, &mut |state| {
            let applied = state
                .apply(&operation, now_ms)
                .map_err(WorkStoreError::Operation)?;
            outcome = Some(applied);
            Ok(())
        });
        match (result, outcome) {
            (Ok(state), Some(outcome)) => ClientResponse::WorkApplyResult(WorkApplyResponseBody {
                id,
                snapshot: state.to_snapshot_dto(),
                outcome,
            }),
            (Ok(_), None) => {
                ClientResponse::error(id, ErrorCode::InternalError, "work state is unavailable")
            }
            (Err(error), _) => map_store_error(id, error),
        }
    }

    fn resolve_context(
        &self,
        session_id: &str,
        context: &MemoryContext,
    ) -> Result<WorkStoreContext, ()> {
        let cwd = context.cwd.as_deref().map(Path::new);
        self.resolver
            .resolve_store_context(session_id, context, cwd)
            .map(|ctx| WorkStoreContext {
                memory_space_id: ctx.memory_space_id,
            })
            .map_err(|_| ())
    }
}

fn invalid(id: String, message: &str) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, message)
}

fn map_store_error(id: String, error: WorkStoreError) -> ClientResponse {
    match error {
        WorkStoreError::InvalidMemorySpace | WorkStoreError::Validation(_) => {
            invalid(id, "invalid work state")
        }
        WorkStoreError::Operation(error) => match error {
            WorkMutationError::StackNotEmpty
            | WorkMutationError::NoActiveWork
            | WorkMutationError::UnsupportedOperation => invalid(id, &error.to_string()),
            WorkMutationError::InvalidOperation(_) => invalid(id, "invalid work operation"),
            WorkMutationError::InvalidState(_) => invalid(id, "invalid work state"),
        },
        WorkStoreError::Corrupt(_) | WorkStoreError::Io(_) | WorkStoreError::Mutation(_) => {
            ClientResponse::error(id, ErrorCode::InternalError, "work state is unavailable")
        }
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
