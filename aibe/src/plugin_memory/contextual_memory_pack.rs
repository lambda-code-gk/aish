//! contextual memory 有効時の pack（注入 + memory RPC）。

use async_trait::async_trait;
use std::sync::Arc;

use super::memory_recipe_service::MemoryRecipeService;
use super::memory_service::MemoryService;
use super::memory_subscribe_service::MemorySubscribeService;
use crate::domain::{AgentTurnContext, ChatMessage, MessageRole};
use crate::ports::outbound::{
    CapabilityPolicy, ContextualMemoryStore, MemoryKindRegistryLoader, MemorySpaceResolver,
    MemorySubscription, MemorySubscriptionBroker, ProfileRegistry, RpcExtension, TurnHook,
    TurnHookError,
};
use aibe_protocol::{
    ClientResponse, MemoryApplyRequestBody, MemoryKindListRequestBody, MemoryQueryRequestBody,
    MemoryRecipeRunRequestBody, MemorySubscribeRequestBody, MEMORY_PROMPT_BUDGET_BYTES,
};

/// `[memory] enabled = true` 時に選ばれる pack。
pub struct ContextualMemoryPack {
    memory_store: Arc<dyn ContextualMemoryStore>,
    memory_space_resolver: Arc<dyn MemorySpaceResolver>,
    memory_kind_registry_loader: Arc<dyn MemoryKindRegistryLoader>,
    memory_broker: Arc<dyn MemorySubscriptionBroker>,
    capability_policy: Arc<dyn CapabilityPolicy>,
    profile_registry: ProfileRegistry,
}

impl ContextualMemoryPack {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        memory_store: Arc<dyn ContextualMemoryStore>,
        memory_space_resolver: Arc<dyn MemorySpaceResolver>,
        memory_kind_registry_loader: Arc<dyn MemoryKindRegistryLoader>,
        memory_broker: Arc<dyn MemorySubscriptionBroker>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        profile_registry: ProfileRegistry,
    ) -> Self {
        Self {
            memory_store,
            memory_space_resolver,
            memory_kind_registry_loader,
            memory_broker,
            capability_policy,
            profile_registry,
        }
    }

    /// system instruction / shell log tail の直後（`agent_turn` が前置した prefix の後）に挿入する。
    fn memory_insert_index(messages: &[ChatMessage]) -> usize {
        let mut idx = 0;
        for m in messages {
            match m.role {
                MessageRole::System => idx += 1,
                MessageRole::User if m.content.starts_with("[shell log tail]\n") => idx += 1,
                _ => break,
            }
        }
        idx
    }

    fn memory_service(&self, broker: Option<Arc<dyn MemorySubscriptionBroker>>) -> MemoryService {
        MemoryService::with_capability_policy(
            Arc::clone(&self.memory_store),
            Arc::clone(&self.memory_space_resolver),
            Arc::clone(&self.memory_kind_registry_loader),
            broker,
            Arc::clone(&self.capability_policy),
        )
    }
}

impl TurnHook for ContextualMemoryPack {
    fn prepare_turn_messages(
        &self,
        context: &AgentTurnContext,
        mut messages: Vec<ChatMessage>,
    ) -> Result<Vec<ChatMessage>, TurnHookError> {
        let Some(ref session_id) = context.ai_session_id else {
            return Ok(messages);
        };
        let user_query = messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let cwd = context.client_cwd.as_ref().map(|c| c.as_path());
        let block = self
            .memory_space_resolver
            .resolve_for_turn(session_id, context.memory_space_id.as_deref(), cwd)
            .ok()
            .and_then(|store_ctx| {
                self.memory_store
                    .resolve_for_prompt(&store_ctx, user_query, MEMORY_PROMPT_BUDGET_BYTES)
                    .ok()
            });
        if let Some(block) = block {
            if !block.content.is_empty() {
                messages.insert(
                    Self::memory_insert_index(&messages),
                    ChatMessage::user(block.content),
                );
            }
        }
        Ok(messages)
    }
}

#[async_trait]
impl RpcExtension for ContextualMemoryPack {
    fn memory_apply(&self, body: MemoryApplyRequestBody) -> ClientResponse {
        self.memory_service(Some(Arc::clone(&self.memory_broker)))
            .apply(body.id, body.session_id, &body.context, body.operation)
    }

    fn memory_query(&self, body: MemoryQueryRequestBody) -> ClientResponse {
        self.memory_service(None)
            .query(body.id, body.session_id, &body.context, body.query)
    }

    fn memory_kind_list(&self, body: MemoryKindListRequestBody) -> ClientResponse {
        self.memory_service(None)
            .kind_list(body.id, body.session_id, &body.context)
    }

    async fn memory_recipe_run(&self, body: MemoryRecipeRunRequestBody) -> ClientResponse {
        let service = MemoryRecipeService::with_capability_policy(
            Arc::clone(&self.memory_store),
            Arc::clone(&self.memory_space_resolver),
            Arc::clone(&self.memory_kind_registry_loader),
            self.profile_registry.clone(),
            Some(Arc::clone(&self.memory_broker)),
            Arc::clone(&self.capability_policy),
        );
        service
            .run(
                body.id,
                body.session_id,
                &body.context,
                &body.recipe,
                body.apply,
                body.user_instruction,
            )
            .await
    }

    fn memory_subscribe_begin(
        &self,
        body: MemorySubscribeRequestBody,
    ) -> (ClientResponse, Option<MemorySubscription>) {
        let service = MemorySubscribeService::with_capability_policy(
            Arc::clone(&self.memory_broker),
            Arc::clone(&self.memory_space_resolver),
            Arc::clone(&self.capability_policy),
        );
        service.begin(body)
    }
}

/// composition root / テスト用: `ContextualMemoryPack` を trait object として返す。
pub fn contextual_pack_arc(
    memory_store: Arc<dyn ContextualMemoryStore>,
    memory_space_resolver: Arc<dyn MemorySpaceResolver>,
    memory_kind_registry_loader: Arc<dyn MemoryKindRegistryLoader>,
    memory_broker: Arc<dyn MemorySubscriptionBroker>,
    capability_policy: Arc<dyn CapabilityPolicy>,
    profile_registry: ProfileRegistry,
) -> (Arc<dyn RpcExtension>, Arc<dyn TurnHook>) {
    let pack = Arc::new(ContextualMemoryPack::new(
        memory_store,
        memory_space_resolver,
        memory_kind_registry_loader,
        memory_broker,
        capability_policy,
        profile_registry,
    ));
    (
        Arc::clone(&pack) as Arc<dyn RpcExtension>,
        Arc::clone(&pack) as Arc<dyn TurnHook>,
    )
}
