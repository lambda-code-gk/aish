#![cfg(feature = "memory")]
//! ContextualMemoryPack TurnHook 注入の回帰。

use std::sync::Arc;

use aibe::adapters::outbound::{
    shared_baseline_recipe_loader, shared_builtin_loader, EmptyContextualMemoryStore,
    InProcessMemorySubscriptionBroker, MockLlm, StaticCapabilityPolicy,
};
use aibe::application::contextual_pack_arc;
use aibe::domain::{AgentTurnContext, ChatMessage, MemoryBlock};
use aibe::ports::outbound::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemorySpaceResolver, ProfileRegistry,
    TurnHook,
};
use aibe_protocol::MemoryContext;

struct NoopMemorySpaceResolver;

impl MemorySpaceResolver for NoopMemorySpaceResolver {
    fn resolve_store_context<'a>(
        &self,
        session_id: &'a str,
        _context: &MemoryContext,
        cwd_path: Option<&'a std::path::Path>,
    ) -> Result<aibe::ports::outbound::MemoryStoreContext<'a>, ContextualMemoryStoreError> {
        Ok(aibe::ports::outbound::MemoryStoreContext {
            session_id,
            memory_space_id: "test".into(),
            cwd: cwd_path,
        })
    }

    fn resolve_for_turn<'a>(
        &self,
        session_id: &'a str,
        explicit_memory_space_id: Option<&str>,
        cwd_path: Option<&'a std::path::Path>,
    ) -> Result<aibe::ports::outbound::MemoryStoreContext<'a>, ContextualMemoryStoreError> {
        Ok(aibe::ports::outbound::MemoryStoreContext {
            session_id,
            memory_space_id: explicit_memory_space_id.unwrap_or("test").to_string(),
            cwd: cwd_path,
        })
    }
}

struct EchoSpaceMemoryStore;

impl ContextualMemoryStore for EchoSpaceMemoryStore {
    fn apply(
        &self,
        _ctx: &aibe::ports::outbound::MemoryStoreContext<'_>,
        _operation: &aibe_protocol::MemoryOperationDto,
        _now_ms: u64,
    ) -> Result<Vec<aibe::domain::MemoryEntry>, ContextualMemoryStoreError> {
        Ok(vec![])
    }

    fn query(
        &self,
        _ctx: &aibe::ports::outbound::MemoryStoreContext<'_>,
        _query: &aibe_protocol::MemoryQueryDto,
    ) -> Result<Vec<aibe::domain::MemoryEntry>, ContextualMemoryStoreError> {
        Ok(vec![])
    }

    fn resolve_for_prompt(
        &self,
        ctx: &aibe::ports::outbound::MemoryStoreContext<'_>,
        _user_query: &str,
        _budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        Ok(MemoryBlock {
            content: format!("[memory from {}]", ctx.memory_space_id),
        })
    }

    fn resolve_for_prompt_explicit(
        &self,
        ctx: &aibe::ports::outbound::MemoryStoreContext<'_>,
        _user_query: &str,
        _budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        Ok(MemoryBlock {
            content: format!("[memory from {}]", ctx.memory_space_id),
        })
    }
}

fn turn_hook(store: Arc<dyn ContextualMemoryStore>) -> Arc<dyn TurnHook> {
    contextual_pack_arc(
        store,
        Arc::new(NoopMemorySpaceResolver),
        shared_builtin_loader(),
        shared_baseline_recipe_loader(),
        Arc::new(InProcessMemorySubscriptionBroker::new()),
        StaticCapabilityPolicy::local_full(),
        ProfileRegistry::single(
            "default",
            Arc::new(MockLlm::new()),
            aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
        ),
        Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
    )
    .1
}

#[test]
fn turn_hook_uses_explicit_memory_space_id() {
    let hook = turn_hook(Arc::new(EchoSpaceMemoryStore));
    let mut ctx = AgentTurnContext::for_text_only(None);
    ctx.ai_session_id = Some("sess_001".into());
    ctx.memory_space_id = Some("ctx_a".into());
    let msgs = hook
        .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
        .expect("hook");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].content, "[memory from ctx_a]");
}

#[test]
fn turn_hook_injects_memory_without_cwd() {
    let hook = turn_hook(Arc::new(EchoSpaceMemoryStore));
    let mut ctx = AgentTurnContext::for_text_only(None);
    ctx.ai_session_id = Some("sess_001".into());
    let msgs = hook
        .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
        .expect("hook");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].content, "[memory from test]");
}

#[test]
fn turn_hook_skips_empty_block() {
    let hook = turn_hook(Arc::new(EmptyContextualMemoryStore));
    let ctx = AgentTurnContext::for_text_only(None);
    let msgs = hook
        .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
        .expect("hook");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "hi");
}
