//! MemoryRecipe RPC ハンドラ。

use std::path::Path;
use std::sync::Arc;

use aibe_protocol::{
    is_valid_session_id, ClientResponse, ErrorCode, MemoryChangeKind, MemoryContext,
    MemoryRecipeProposalDto, MemoryRecipeStatus,
};

use crate::domain::{
    build_clarify_goal_messages, builtin_memory_kind_registry, collect_clarify_goal_materials,
    parse_and_validate_recipe_output, publish_memory_changes, MemoryRecipeError,
    RECIPE_CLARIFY_GOAL,
};
use crate::ports::outbound::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemorySpaceResolver,
    MemorySubscriptionBroker, ProfileRegistry,
};

pub struct MemoryRecipeService {
    store: Arc<dyn ContextualMemoryStore>,
    resolver: Arc<dyn MemorySpaceResolver>,
    profile_registry: ProfileRegistry,
    broker: Option<Arc<dyn MemorySubscriptionBroker>>,
}

impl MemoryRecipeService {
    pub fn new(
        store: Arc<dyn ContextualMemoryStore>,
        resolver: Arc<dyn MemorySpaceResolver>,
        profile_registry: ProfileRegistry,
    ) -> Self {
        Self {
            store,
            resolver,
            profile_registry,
            broker: None,
        }
    }

    pub fn with_broker(
        store: Arc<dyn ContextualMemoryStore>,
        resolver: Arc<dyn MemorySpaceResolver>,
        profile_registry: ProfileRegistry,
        broker: Arc<dyn MemorySubscriptionBroker>,
    ) -> Self {
        Self {
            store,
            resolver,
            profile_registry,
            broker: Some(broker),
        }
    }

    pub async fn run(
        &self,
        id: String,
        session_id: String,
        context: &MemoryContext,
        recipe: &str,
        apply: bool,
        user_instruction: Option<String>,
    ) -> ClientResponse {
        if let Err(msg) = validate_session_id(&session_id) {
            return invalid(id, msg);
        }
        if context.cwd.as_deref().is_none_or(str::is_empty) {
            return invalid(id, "cwd is required for memory recipes");
        }
        let cwd_path = context.cwd.as_deref().map(Path::new);
        let store_ctx = match self
            .resolver
            .resolve_store_context(&session_id, context, cwd_path)
        {
            Ok(ctx) => ctx,
            Err(e) => return map_store_error(id, e),
        };

        if recipe != RECIPE_CLARIFY_GOAL {
            return invalid(id, &format!("unknown recipe: {recipe}"));
        }

        let materials = match collect_clarify_goal_materials(self.store.as_ref(), &store_ctx) {
            Ok(m) => m,
            Err(e) => return map_recipe_error(id, e),
        };

        let (system, user) = build_clarify_goal_messages(&materials, user_instruction.as_deref());
        let messages = vec![
            crate::domain::ChatMessage::system(system),
            crate::domain::ChatMessage::user(user),
        ];

        let (llm, _capability) = match self.profile_registry.resolve(None) {
            Ok(pair) => pair,
            Err(msg) => return invalid(id, &msg),
        };

        let assistant = match llm.complete(&messages).await {
            Ok(msg) => msg,
            Err(e) => {
                return ClientResponse::error(
                    id,
                    ErrorCode::InvalidRequest,
                    format!("recipe llm failed: {e}"),
                );
            }
        };

        let registry = builtin_memory_kind_registry();
        let validated = match parse_and_validate_recipe_output(&assistant.content, registry) {
            Ok(v) => v,
            Err(e) => return map_recipe_error(id, e),
        };

        let proposals: Vec<MemoryRecipeProposalDto> = validated
            .proposals
            .iter()
            .map(|p| MemoryRecipeProposalDto {
                operation: p.operation.clone(),
                rationale: p.rationale.clone(),
            })
            .collect();

        if !apply {
            return ClientResponse::MemoryRecipeRunResult {
                id,
                status: MemoryRecipeStatus::Proposed,
                summary: validated.summary,
                proposals,
                applied_entries: vec![],
            };
        }

        let now_ms = current_time_ms();
        let mut applied_entries = Vec::new();
        let mut applied_domain_entries = Vec::new();
        for proposal in &validated.proposals {
            match self.store.apply(&store_ctx, &proposal.operation, now_ms) {
                Ok(entries) => {
                    applied_entries.extend(entries.iter().map(|e| e.to_dto()));
                    applied_domain_entries.extend(entries);
                }
                Err(e) => return map_store_error(id, e),
            }
        }

        if let Some(broker) = &self.broker {
            publish_memory_changes(
                broker.as_ref(),
                &store_ctx.memory_space_id,
                MemoryChangeKind::RecipeApplied,
                &applied_domain_entries,
            );
        }

        ClientResponse::MemoryRecipeRunResult {
            id,
            status: MemoryRecipeStatus::Applied,
            summary: validated.summary,
            proposals,
            applied_entries,
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

fn map_store_error(id: String, err: ContextualMemoryStoreError) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, err.to_string())
}

fn map_recipe_error(id: String, err: MemoryRecipeError) -> ClientResponse {
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
