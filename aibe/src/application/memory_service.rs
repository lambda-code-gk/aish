//! contextual memory RPC ハンドラ。

use std::path::Path;

use aibe_protocol::{
    is_valid_session_id, ClientResponse, ErrorCode, MemoryApplyStatus, MemoryContext,
    MemoryKindDefinitionDto, MemoryOperationDto, MemoryQueryDto, MemoryQueryStatus, MemoryScopeDto,
};

use crate::domain::{
    change_kind_for_operation, publish_memory_changes, resolve_memory_operation_add,
    MemoryValidationError,
};
use crate::ports::outbound::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemorySpaceResolver,
    MemorySubscriptionBroker,
};

pub struct MemoryService {
    store: std::sync::Arc<dyn ContextualMemoryStore>,
    resolver: std::sync::Arc<dyn MemorySpaceResolver>,
    broker: Option<std::sync::Arc<dyn MemorySubscriptionBroker>>,
}

impl MemoryService {
    pub fn new(
        store: std::sync::Arc<dyn ContextualMemoryStore>,
        resolver: std::sync::Arc<dyn MemorySpaceResolver>,
    ) -> Self {
        Self {
            store,
            resolver,
            broker: None,
        }
    }

    pub fn with_broker(
        store: std::sync::Arc<dyn ContextualMemoryStore>,
        resolver: std::sync::Arc<dyn MemorySpaceResolver>,
        broker: std::sync::Arc<dyn MemorySubscriptionBroker>,
    ) -> Self {
        Self {
            store,
            resolver,
            broker: Some(broker),
        }
    }

    pub fn apply(
        &self,
        id: String,
        session_id: String,
        context: &MemoryContext,
        operation: MemoryOperationDto,
    ) -> ClientResponse {
        if let Err(msg) = validate_session_id(&session_id) {
            return invalid(id, msg);
        }
        let operation = match normalize_operation(operation) {
            Ok(op) => op,
            Err(e) => return map_validation_error(id, e),
        };
        if let Err(msg) = validate_cwd_for_operation(context, &operation) {
            return invalid(id, msg);
        }
        let cwd_path = context.cwd.as_deref().map(Path::new);
        let store_ctx = match self
            .resolver
            .resolve_store_context(&session_id, context, cwd_path)
        {
            Ok(ctx) => ctx,
            Err(e) => return map_store_error(id, e),
        };
        let now_ms = current_time_ms();
        match self.store.apply(&store_ctx, &operation, now_ms) {
            Ok(entries) => {
                if let Some(broker) = &self.broker {
                    publish_memory_changes(
                        broker.as_ref(),
                        &store_ctx.memory_space_id,
                        change_kind_for_operation(&operation),
                        &entries,
                    );
                }
                ClientResponse::MemoryApplyResult {
                    id,
                    status: MemoryApplyStatus::Ok,
                    entries: entries.iter().map(|e| e.to_dto()).collect(),
                }
            }
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
        if let Err(msg) = validate_session_id(&session_id) {
            return invalid(id, msg);
        }
        if let Err(msg) = validate_cwd_for_query(context, &query) {
            return invalid(id, msg);
        }
        let cwd_path = context.cwd.as_deref().map(Path::new);
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

    pub fn kind_list(
        &self,
        id: String,
        session_id: String,
        context: &MemoryContext,
    ) -> ClientResponse {
        if let Err(msg) = validate_session_id(&session_id) {
            return invalid(id, msg);
        }
        let _ = context;
        let kinds = crate::domain::builtin_memory_kind_registry()
            .list_definitions()
            .into_iter()
            .map(MemoryKindDefinitionDto::from)
            .collect();
        ClientResponse::MemoryKindListResult {
            id,
            status: MemoryQueryStatus::Ok,
            kinds,
        }
    }
}

fn validate_session_id(session_id: &str) -> Result<(), &'static str> {
    if is_valid_session_id(session_id) {
        Ok(())
    } else {
        Err("invalid session_id")
    }
}

fn normalize_operation(
    operation: MemoryOperationDto,
) -> Result<MemoryOperationDto, MemoryValidationError> {
    match operation {
        MemoryOperationDto::Add(add) => {
            let resolved = resolve_memory_operation_add(&add)?;
            Ok(MemoryOperationDto::Add(resolved))
        }
        other => Ok(other),
    }
}

fn validate_cwd_for_operation(
    context: &MemoryContext,
    operation: &MemoryOperationDto,
) -> Result<(), &'static str> {
    let needs_cwd = match operation {
        MemoryOperationDto::Add(add) => add.scope == Some(MemoryScopeDto::Project),
        MemoryOperationDto::ClearKind(clear) => clear.scope == MemoryScopeDto::Project,
        MemoryOperationDto::Archive(_) => false,
    };
    if needs_cwd && context.cwd.as_deref().is_none_or(str::is_empty) {
        return Err("cwd is required for project-scoped memory");
    }
    Ok(())
}

fn validate_cwd_for_query(
    context: &MemoryContext,
    query: &MemoryQueryDto,
) -> Result<(), &'static str> {
    if query.scope == Some(MemoryScopeDto::Project)
        && context.cwd.as_deref().is_none_or(str::is_empty)
    {
        return Err("cwd is required for project-scoped memory");
    }
    Ok(())
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

fn map_validation_error(id: String, err: MemoryValidationError) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, err.to_string())
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
