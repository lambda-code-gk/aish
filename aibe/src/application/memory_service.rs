//! contextual memory RPC ハンドラ。

use std::path::Path;

use aibe_protocol::{
    ClientResponse, ErrorCode, MemoryApplyStatus, MemoryContext, MemoryOperationDto,
    MemoryQueryDto, MemoryQueryStatus,
};

use crate::domain::MemoryValidationError;
use crate::ports::outbound::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemorySpaceResolver,
};

pub struct MemoryService {
    store: std::sync::Arc<dyn ContextualMemoryStore>,
    resolver: std::sync::Arc<dyn MemorySpaceResolver>,
}

impl MemoryService {
    pub fn new(
        store: std::sync::Arc<dyn ContextualMemoryStore>,
        resolver: std::sync::Arc<dyn MemorySpaceResolver>,
    ) -> Self {
        Self { store, resolver }
    }

    pub fn apply(
        &self,
        id: String,
        session_id: String,
        context: &MemoryContext,
        operation: MemoryOperationDto,
    ) -> ClientResponse {
        if session_id.is_empty() {
            return invalid(id, "session_id must not be empty");
        }
        if context.cwd.is_empty() {
            return invalid(id, "cwd must not be empty");
        }
        let cwd_path = Path::new(&context.cwd);
        let store_ctx = match self
            .resolver
            .resolve_store_context(&session_id, context, cwd_path)
        {
            Ok(ctx) => ctx,
            Err(e) => return map_store_error(id, e),
        };
        let now_ms = current_time_ms();
        match self.store.apply(&store_ctx, &operation, now_ms) {
            Ok(entries) => ClientResponse::MemoryApplyResult {
                id,
                status: MemoryApplyStatus::Ok,
                entries: entries.iter().map(|e| e.to_dto()).collect(),
            },
            Err(e) => map_store_error(id, e),
        }
    }

    pub fn query(
        &self,
        id: String,
        session_id: String,
        context: &MemoryContext,
        query: MemoryQueryDto,
    ) -> ClientResponse {
        if session_id.is_empty() {
            return invalid(id, "session_id must not be empty");
        }
        if context.cwd.is_empty() {
            return invalid(id, "cwd must not be empty");
        }
        let cwd_path = Path::new(&context.cwd);
        let store_ctx = match self
            .resolver
            .resolve_store_context(&session_id, context, cwd_path)
        {
            Ok(ctx) => ctx,
            Err(e) => return map_store_error(id, e),
        };
        match self.store.query(&store_ctx, &query) {
            Ok(entries) => {
                let prompt_block = if query.include_prompt_block {
                    let user_query = query.user_query.as_deref().unwrap_or("");
                    self.store
                        .resolve_for_prompt(
                            &store_ctx,
                            user_query,
                            aibe_protocol::MEMORY_PROMPT_BUDGET_BYTES,
                        )
                        .ok()
                        .map(|b| b.content)
                        .filter(|s| !s.is_empty())
                } else {
                    None
                };
                ClientResponse::MemoryQueryResult {
                    id,
                    status: MemoryQueryStatus::Ok,
                    entries: entries.iter().map(|e| e.to_dto()).collect(),
                    prompt_block,
                }
            }
            Err(e) => map_store_error(id, e),
        }
    }
}

fn map_store_error(id: String, err: ContextualMemoryStoreError) -> ClientResponse {
    let message = match &err {
        ContextualMemoryStoreError::Validation(MemoryValidationError::VersionConflict) => {
            "version_conflict".to_string()
        }
        _ => err.to_string(),
    };
    ClientResponse::error(id, ErrorCode::InvalidRequest, message)
}

fn invalid(id: String, message: &str) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, message)
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
