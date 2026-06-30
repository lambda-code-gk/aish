#![cfg(feature = "memory")]
//! ContextualMemoryPack TurnHook 注入の回帰。

use std::sync::Arc;

use aibe::adapters::outbound::{
    shared_baseline_recipe_loader, shared_builtin_loader, EmptyContextualMemoryStore,
    InProcessMemorySubscriptionBroker, MockLlm, StaticCapabilityPolicy,
};
use aibe::application::{contextual_pack_arc, contextual_pack_with_work_arc};
use aibe::domain::{
    AgentTurnContext, ChatMessage, MemoryBlock, WorkEntry, WorkEntryKind, WorkItem, WorkState,
    WorkStatus,
};
use aibe::ports::outbound::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemorySpaceResolver, ProfileRegistry,
    TurnHook, WorkStore, WorkStoreError,
};
use aibe_protocol::{MemoryContext, WORK_SCHEMA_VERSION};

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

struct FixedWorkStore {
    state: WorkState,
}

impl FixedWorkStore {
    fn new(state: WorkState) -> Self {
        Self { state }
    }
}

impl WorkStore for FixedWorkStore {
    fn load(
        &self,
        _ctx: &aibe::ports::outbound::WorkStoreContext,
    ) -> Result<WorkState, WorkStoreError> {
        Ok(self.state.clone())
    }

    fn mutate(
        &self,
        _ctx: &aibe::ports::outbound::WorkStoreContext,
        _mutation: &mut dyn FnMut(&mut WorkState) -> Result<(), WorkStoreError>,
    ) -> Result<WorkState, WorkStoreError> {
        Err(WorkStoreError::Mutation(
            "not used in turn hook tests".into(),
        ))
    }
}

#[derive(Clone)]
enum WorkStoreFailure {
    InvalidMemorySpace,
    Corrupt(String),
    Validation(String),
    Io(String),
    Mutation(String),
}

struct FailingWorkStore {
    error: WorkStoreFailure,
}

impl WorkStore for FailingWorkStore {
    fn load(
        &self,
        _ctx: &aibe::ports::outbound::WorkStoreContext,
    ) -> Result<WorkState, WorkStoreError> {
        Err(self.error.clone().into())
    }

    fn mutate(
        &self,
        _ctx: &aibe::ports::outbound::WorkStoreContext,
        _mutation: &mut dyn FnMut(&mut WorkState) -> Result<(), WorkStoreError>,
    ) -> Result<WorkState, WorkStoreError> {
        Err(self.error.clone().into())
    }
}

impl From<WorkStoreFailure> for WorkStoreError {
    fn from(value: WorkStoreFailure) -> Self {
        match value {
            WorkStoreFailure::InvalidMemorySpace => WorkStoreError::InvalidMemorySpace,
            WorkStoreFailure::Corrupt(message) => WorkStoreError::Corrupt(message),
            WorkStoreFailure::Validation(message) => WorkStoreError::Validation(message),
            WorkStoreFailure::Io(message) => WorkStoreError::Io(message),
            WorkStoreFailure::Mutation(message) => WorkStoreError::Mutation(message),
        }
    }
}

struct EchoBudgetMemoryStore {
    prefix: String,
}

impl ContextualMemoryStore for EchoBudgetMemoryStore {
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
        _ctx: &aibe::ports::outbound::MemoryStoreContext<'_>,
        _user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        let mut content = self.prefix.clone();
        if budget_bytes > content.len() {
            content.push_str(&"m".repeat(budget_bytes - content.len()));
        } else {
            content.truncate(budget_bytes);
        }
        Ok(MemoryBlock { content })
    }

    fn resolve_for_prompt_explicit(
        &self,
        ctx: &aibe::ports::outbound::MemoryStoreContext<'_>,
        user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        self.resolve_for_prompt(ctx, user_query, budget_bytes)
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

fn turn_hook_with_work_store(
    store: Arc<dyn ContextualMemoryStore>,
    work_store: Arc<dyn WorkStore>,
) -> Arc<dyn TurnHook> {
    contextual_pack_with_work_arc(
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
        work_store,
    )
    .1
}

fn active_work_state() -> WorkState {
    WorkState {
        schema_version: WORK_SCHEMA_VERSION,
        revision: 7,
        next_work_id: 6,
        active_work_id: Some(2),
        stack: vec![1],
        works: vec![
            WorkItem {
                id: 1,
                title: "parent work".into(),
                goal: "parent work".into(),
                status: WorkStatus::Paused,
                parent_id: None,
                created_at_ms: 1,
                updated_at_ms: 1,
                finished_at_ms: None,
                focus: None,
                summary: None,
            },
            WorkItem {
                id: 2,
                title: "current goal".into(),
                goal: "current goal".into(),
                status: WorkStatus::Active,
                parent_id: Some(1),
                created_at_ms: 2,
                updated_at_ms: 2,
                finished_at_ms: None,
                focus: Some("current focus".into()),
                summary: None,
            },
            WorkItem {
                id: 3,
                title: "deferred task".into(),
                goal: "deferred task".into(),
                status: WorkStatus::Deferred,
                parent_id: None,
                created_at_ms: 3,
                updated_at_ms: 3,
                finished_at_ms: None,
                focus: None,
                summary: None,
            },
            WorkItem {
                id: 4,
                title: "done task".into(),
                goal: "done task".into(),
                status: WorkStatus::Done,
                parent_id: None,
                created_at_ms: 4,
                updated_at_ms: 4,
                finished_at_ms: Some(4),
                focus: None,
                summary: None,
            },
        ],
        entries: vec![
            WorkEntry {
                id: 1,
                work_id: 2,
                kind: WorkEntryKind::Note,
                text: "active note".into(),
                created_at_ms: 1,
            },
            WorkEntry {
                id: 2,
                work_id: 2,
                kind: WorkEntryKind::Idea,
                text: "active idea".into(),
                created_at_ms: 2,
            },
            WorkEntry {
                id: 3,
                work_id: 2,
                kind: WorkEntryKind::Decision,
                text: "decision 1".into(),
                created_at_ms: 3,
            },
            WorkEntry {
                id: 4,
                work_id: 2,
                kind: WorkEntryKind::Decision,
                text: "decision 2".into(),
                created_at_ms: 4,
            },
            WorkEntry {
                id: 5,
                work_id: 2,
                kind: WorkEntryKind::Decision,
                text: "decision 3".into(),
                created_at_ms: 5,
            },
            WorkEntry {
                id: 6,
                work_id: 2,
                kind: WorkEntryKind::Decision,
                text: "decision 4".into(),
                created_at_ms: 6,
            },
            WorkEntry {
                id: 7,
                work_id: 3,
                kind: WorkEntryKind::Decision,
                text: "deferred decision".into(),
                created_at_ms: 7,
            },
        ],
    }
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

#[test]
fn work_turn_hook_injects_active_goal_focus_and_recent_decisions() {
    let hook = turn_hook_with_work_store(
        Arc::new(EmptyContextualMemoryStore),
        Arc::new(FixedWorkStore::new(active_work_state())),
    );
    let mut ctx = AgentTurnContext::for_text_only(None);
    ctx.ai_session_id = Some("sess_001".into());
    let msgs = hook
        .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
        .expect("hook");
    assert_eq!(msgs.len(), 2);
    let work = &msgs[0].content;
    assert!(work.contains("[active work]"));
    assert!(work.contains("current goal"));
    assert!(work.contains("current focus"));
    assert!(work.contains("decision 4"));
    assert!(work.contains("decision 3"));
    assert!(work.contains("decision 2"));
    assert!(!work.contains("decision 1"));
    assert!(!work.contains("active note"));
    assert!(!work.contains("active idea"));
    assert!(!work.contains("deferred task"));
    assert!(!work.contains("done task"));
    assert_eq!(msgs[1].content, "hi");
}

#[test]
fn work_and_memory_injection_share_existing_budget() {
    let hook = turn_hook_with_work_store(
        Arc::new(EchoBudgetMemoryStore {
            prefix: String::new(),
        }),
        Arc::new(FixedWorkStore::new(WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 1,
            next_work_id: 2,
            active_work_id: Some(1),
            stack: Vec::new(),
            works: vec![WorkItem {
                id: 1,
                title: "goal".into(),
                goal: "goal".into(),
                status: WorkStatus::Active,
                parent_id: None,
                created_at_ms: 1,
                updated_at_ms: 1,
                finished_at_ms: None,
                focus: Some("focus".into()),
                summary: None,
            }],
            entries: vec![WorkEntry {
                id: 1,
                work_id: 1,
                kind: WorkEntryKind::Decision,
                text: "decision".into(),
                created_at_ms: 1,
            }],
        })),
    );
    let mut ctx = AgentTurnContext::for_text_only(None);
    ctx.ai_session_id = Some("sess_001".into());
    let msgs = hook
        .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
        .expect("hook");
    assert_eq!(msgs.len(), 3);
    let injected_total = msgs[0].content.len() + msgs[1].content.len();
    assert_eq!(injected_total, aibe_protocol::MEMORY_PROMPT_BUDGET_BYTES);
    assert_eq!(msgs[2].content, "hi");
}

#[test]
fn work_turn_hook_excludes_non_active_and_non_required_fields() {
    let hook = turn_hook_with_work_store(
        Arc::new(EmptyContextualMemoryStore),
        Arc::new(FixedWorkStore::new(WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 9,
            next_work_id: 6,
            active_work_id: Some(2),
            stack: vec![1],
            works: vec![
                WorkItem {
                    id: 1,
                    title: "paused work".into(),
                    goal: "paused work".into(),
                    status: WorkStatus::Paused,
                    parent_id: None,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    finished_at_ms: None,
                    focus: None,
                    summary: None,
                },
                WorkItem {
                    id: 2,
                    title: "active goal".into(),
                    goal: "active goal".into(),
                    status: WorkStatus::Active,
                    parent_id: Some(1),
                    created_at_ms: 2,
                    updated_at_ms: 2,
                    finished_at_ms: None,
                    focus: None,
                    summary: None,
                },
                WorkItem {
                    id: 3,
                    title: "deferred task".into(),
                    goal: "deferred task".into(),
                    status: WorkStatus::Deferred,
                    parent_id: None,
                    created_at_ms: 3,
                    updated_at_ms: 3,
                    finished_at_ms: None,
                    focus: None,
                    summary: None,
                },
                WorkItem {
                    id: 4,
                    title: "done task".into(),
                    goal: "done task".into(),
                    status: WorkStatus::Done,
                    parent_id: None,
                    created_at_ms: 4,
                    updated_at_ms: 4,
                    finished_at_ms: Some(4),
                    focus: None,
                    summary: None,
                },
            ],
            entries: vec![
                WorkEntry {
                    id: 1,
                    work_id: 1,
                    kind: WorkEntryKind::Decision,
                    text: "paused decision".into(),
                    created_at_ms: 1,
                },
                WorkEntry {
                    id: 2,
                    work_id: 2,
                    kind: WorkEntryKind::Decision,
                    text: "active decision".into(),
                    created_at_ms: 2,
                },
                WorkEntry {
                    id: 3,
                    work_id: 2,
                    kind: WorkEntryKind::Note,
                    text: "active note".into(),
                    created_at_ms: 3,
                },
                WorkEntry {
                    id: 4,
                    work_id: 2,
                    kind: WorkEntryKind::Idea,
                    text: "active idea".into(),
                    created_at_ms: 4,
                },
            ],
        })),
    );
    let mut ctx = AgentTurnContext::for_text_only(None);
    ctx.ai_session_id = Some("sess_001".into());
    let msgs = hook
        .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
        .expect("hook");
    let work = &msgs[0].content;
    assert!(work.contains("active goal"));
    assert!(work.contains("active decision"));
    assert!(!work.contains("paused work"));
    assert!(!work.contains("paused decision"));
    assert!(!work.contains("deferred task"));
    assert!(!work.contains("done task"));
    assert!(!work.contains("active note"));
    assert!(!work.contains("active idea"));
}

#[test]
fn work_turn_hook_is_best_effort_for_missing_or_corrupt_state() {
    let hook = turn_hook_with_work_store(
        Arc::new(EchoSpaceMemoryStore),
        Arc::new(FailingWorkStore {
            error: WorkStoreFailure::Corrupt("broken".into()),
        }),
    );
    let mut ctx = AgentTurnContext::for_text_only(None);
    ctx.ai_session_id = Some("sess_001".into());
    let msgs = hook
        .prepare_turn_messages(&ctx, vec![ChatMessage::user("hi")])
        .expect("hook");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].content, "[memory from test]");
    assert_eq!(msgs[1].content, "hi");
}
