//! MemoryRecipe RPC ハンドラ。

use std::path::Path;
use std::sync::Arc;

use aibe_protocol::{
    is_valid_session_id, ClientResponse, ErrorCode, MemoryChangeKind, MemoryContext,
    MemoryRecipeProposalDto, MemoryRecipeStatus,
};

use crate::application::llm_call_trace::trace_llm_result;
use crate::domain::{
    build_recipe_messages, collect_recipe_materials, parse_and_validate_recipe_output,
    publish_memory_changes, required_capabilities_for_memory_operations, Capability,
    MemoryKindRegistryError, MemoryRecipeError, MemoryRecipeRegistryError,
};
use crate::ports::outbound::{
    CapabilityPolicy, ContextualMemoryStore, ContextualMemoryStoreError, LlmCallTracer,
    MemoryKindRegistryLoader, MemoryRecipeRegistryLoader, MemorySpaceResolver,
    MemorySubscriptionBroker, NoopLlmCallTracer, ProfileRegistry,
};

pub struct MemoryRecipeService {
    store: Arc<dyn ContextualMemoryStore>,
    resolver: Arc<dyn MemorySpaceResolver>,
    registry_loader: Arc<dyn MemoryKindRegistryLoader>,
    recipe_registry_loader: Arc<dyn MemoryRecipeRegistryLoader>,
    profile_registry: ProfileRegistry,
    broker: Option<Arc<dyn MemorySubscriptionBroker>>,
    capability_policy: Arc<dyn CapabilityPolicy>,
    llm_tracer: Arc<dyn LlmCallTracer>,
}

impl MemoryRecipeService {
    pub fn new(
        store: Arc<dyn ContextualMemoryStore>,
        resolver: Arc<dyn MemorySpaceResolver>,
        registry_loader: Arc<dyn MemoryKindRegistryLoader>,
        recipe_registry_loader: Arc<dyn MemoryRecipeRegistryLoader>,
        profile_registry: ProfileRegistry,
    ) -> Self {
        Self::with_capability_policy(
            store,
            resolver,
            registry_loader,
            recipe_registry_loader,
            profile_registry,
            None,
            crate::adapters::outbound::StaticCapabilityPolicy::local_full(),
            Arc::new(NoopLlmCallTracer),
        )
    }

    pub fn with_broker(
        store: Arc<dyn ContextualMemoryStore>,
        resolver: Arc<dyn MemorySpaceResolver>,
        registry_loader: Arc<dyn MemoryKindRegistryLoader>,
        recipe_registry_loader: Arc<dyn MemoryRecipeRegistryLoader>,
        profile_registry: ProfileRegistry,
        broker: Arc<dyn MemorySubscriptionBroker>,
    ) -> Self {
        Self::with_capability_policy(
            store,
            resolver,
            registry_loader,
            recipe_registry_loader,
            profile_registry,
            Some(broker),
            crate::adapters::outbound::StaticCapabilityPolicy::local_full(),
            Arc::new(NoopLlmCallTracer),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_capability_policy(
        store: Arc<dyn ContextualMemoryStore>,
        resolver: Arc<dyn MemorySpaceResolver>,
        registry_loader: Arc<dyn MemoryKindRegistryLoader>,
        recipe_registry_loader: Arc<dyn MemoryRecipeRegistryLoader>,
        profile_registry: ProfileRegistry,
        broker: Option<Arc<dyn MemorySubscriptionBroker>>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        llm_tracer: Arc<dyn LlmCallTracer>,
    ) -> Self {
        Self {
            store,
            resolver,
            registry_loader,
            recipe_registry_loader,
            profile_registry,
            broker,
            capability_policy,
            llm_tracer,
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
        if let Err(denied) = self.capability_policy.require(Capability::MemoryRecipeRun) {
            return capability_denied(id, denied);
        }
        if let Err(denied) = self.capability_policy.require(Capability::MemoryRead) {
            return capability_denied(id, denied);
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

        let recipe_registry = match self.recipe_registry_loader.load_strict() {
            Ok(registry) => registry,
            Err(e) => return map_recipe_registry_error(id, e),
        };
        let recipe_def = match recipe_registry.get(recipe) {
            Some(def) => def.clone(),
            None => return invalid(id, &format!("unknown recipe: {recipe}")),
        };

        let materials = match collect_recipe_materials(self.store.as_ref(), &store_ctx, &recipe_def)
        {
            Ok(m) => m,
            Err(e) => return map_recipe_error(id, e),
        };

        let (system, user) =
            build_recipe_messages(&recipe_def, &materials, user_instruction.as_deref());
        let messages = vec![
            crate::domain::ChatMessage::system(system),
            crate::domain::ChatMessage::user(user),
        ];

        let profile_name = recipe_def.llm_profile.as_deref();
        let (llm, _capability) = match self.profile_registry.resolve(profile_name) {
            Ok(pair) => pair,
            Err(msg) => return invalid(id, &msg),
        };

        let assistant =
            match trace_llm_result(&self.llm_tracer, "memory_recipe", profile_name, || {
                llm.complete(&messages)
            })
            .await
            {
                Ok(msg) => msg,
                Err(e) => {
                    return ClientResponse::error(
                        id,
                        ErrorCode::InvalidRequest,
                        format!("recipe llm failed: {e}"),
                    );
                }
            };

        let registry = match self
            .registry_loader
            .load_strict(store_ctx.memory_space_id.as_str())
        {
            Ok(registry) => registry,
            Err(e) => return map_registry_error(id, e),
        };
        let validated = match parse_and_validate_recipe_output(
            &assistant.content,
            &registry,
            &recipe_def.output.allow_operations,
        ) {
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

        for cap in required_capabilities_for_memory_operations(
            validated.proposals.iter().map(|p| &p.operation),
        ) {
            if let Err(denied) = self.capability_policy.require(cap) {
                return capability_denied(id, denied);
            }
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

fn map_recipe_registry_error(id: String, err: MemoryRecipeRegistryError) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, err.to_string())
}

fn map_registry_error(id: String, err: MemoryKindRegistryError) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, err.to_string())
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

fn capability_denied(
    id: String,
    denied: crate::ports::outbound::CapabilityDenied,
) -> ClientResponse {
    ClientResponse::error(id, ErrorCode::InvalidRequest, denied.message())
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
