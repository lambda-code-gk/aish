//! Work RPC application service。

use std::path::Path;
use std::sync::Arc;

use aibe_protocol::{
    is_valid_session_id, ClientResponse, ErrorCode, MemoryContext, WorkOperationDto,
    WorkQueryResponseBody,
};

use crate::domain::Capability;
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
        if self.resolve_context(&session_id, context).is_err() {
            return invalid(id, "invalid work context");
        }
        invalid(id, "work mutations are not available in phase 0")
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
        WorkStoreError::Corrupt(_) | WorkStoreError::Io(_) | WorkStoreError::Mutation(_) => {
            ClientResponse::error(id, ErrorCode::InternalError, "work state is unavailable")
        }
    }
}
