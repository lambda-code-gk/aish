//! contextual memory RPC ハンドラ。

use std::path::Path;
use std::sync::Arc;

use aibe_protocol::{
    ClientResponse, ErrorCode, MemoryApplyStatus, MemoryOperationDto, MemoryQueryDto,
    MemoryQueryStatus,
};

use crate::domain::MemoryValidationError;
use crate::ports::outbound::{ContextualMemoryStore, ContextualMemoryStoreError};

pub struct MemoryService {
    store: Arc<dyn ContextualMemoryStore>,
}

impl MemoryService {
    pub fn new(store: Arc<dyn ContextualMemoryStore>) -> Self {
        Self { store }
    }

    pub fn apply(
        &self,
        id: String,
        session_id: String,
        cwd: &str,
        operation: MemoryOperationDto,
    ) -> ClientResponse {
        if session_id.is_empty() {
            return invalid(id, "session_id must not be empty");
        }
        if cwd.is_empty() {
            return invalid(id, "cwd must not be empty");
        }
        let cwd_path = Path::new(cwd);
        let now_ms = current_time_ms();
        match self
            .store
            .apply(&session_id, Some(cwd_path), &operation, now_ms)
        {
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
        cwd: &str,
        query: MemoryQueryDto,
    ) -> ClientResponse {
        if session_id.is_empty() {
            return invalid(id, "session_id must not be empty");
        }
        if cwd.is_empty() {
            return invalid(id, "cwd must not be empty");
        }
        let cwd_path = Path::new(cwd);
        match self.store.query(&session_id, Some(cwd_path), &query) {
            Ok(entries) => {
                let prompt_block = if query.include_prompt_block {
                    let user_query = query.user_query.as_deref().unwrap_or("");
                    self.store
                        .resolve_for_prompt(
                            &session_id,
                            Some(cwd_path),
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
