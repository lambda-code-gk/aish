//! contextual memory store と prompt 注入の統合テスト。

use aibe::adapters::outbound::FilesystemContextualMemoryStore;
use aibe::application::memory_service::MemoryService;
use aibe::domain::MemoryStatus;
use aibe::ports::outbound::{ContextualMemoryStore, MemoryStoreContext};
use aibe_protocol::{
    MemoryContext, MemoryInjectPolicyDto, MemoryOperationAdd, MemoryOperationClearKind,
    MemoryOperationDto, MemoryQueryDto, MemoryScopeDto, MemoryStatusDto,
    MEMORY_PROMPT_BUDGET_BYTES,
};
use std::sync::Arc;
use tempfile::tempdir;

fn ctx<'a>(
    session_id: &'a str,
    memory_space_id: &str,
    cwd: &'a std::path::Path,
) -> MemoryStoreContext<'a> {
    MemoryStoreContext {
        session_id,
        memory_space_id: memory_space_id.to_string(),
        cwd: Some(cwd),
    }
}

fn goal_op(text: &str) -> MemoryOperationDto {
    MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "goal".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::Pinned,
        status: MemoryStatusDto::Active,
        text: text.into(),
        make_active: true,
    })
}

fn now_op(text: &str) -> MemoryOperationDto {
    MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "now".into(),
        scope: MemoryScopeDto::Session,
        inject: MemoryInjectPolicyDto::Pinned,
        status: MemoryStatusDto::Active,
        text: text.into(),
        make_active: true,
    })
}

#[test]
fn resolve_for_prompt_injects_goal_now_not_idea_on_normal_query() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c = ctx("sess", "ctx_a", &cwd);
    store.apply(&c, &goal_op("build memory"), 1).expect("goal");
    store.apply(&c, &now_op("wire protocol"), 2).expect("now");
    let idea = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "idea".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::OnDemand,
        status: MemoryStatusDto::Open,
        text: "later idea".into(),
        make_active: false,
    });
    store.apply(&c, &idea, 3).expect("idea");

    let block = store
        .resolve_for_prompt(&c, "fix rust error", MEMORY_PROMPT_BUDGET_BYTES)
        .expect("resolve");
    assert!(block.content.contains("build memory"));
    assert!(block.content.contains("wire protocol"));
    assert!(!block.content.contains("later idea"));
}

#[test]
fn resolve_for_prompt_includes_idea_on_mvp_query() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c = ctx("sess", "ctx_a", &cwd);
    let idea = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "idea".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::OnDemand,
        status: MemoryStatusDto::Open,
        text: "card idea".into(),
        make_active: false,
    });
    store.apply(&c, &idea, 1).expect("idea");

    let block = store
        .resolve_for_prompt(
            &c,
            "今あるideaからMVPを整理して",
            MEMORY_PROMPT_BUDGET_BYTES,
        )
        .expect("resolve");
    assert!(block.content.contains("card idea"));
}

#[test]
fn memory_request_without_memory_space_id_does_not_fail() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");

    let resolver = aibe::adapters::outbound::FilesystemMemorySpaceResolver;
    let service = MemoryService::new(Arc::new(store), Arc::new(resolver));
    // 旧クライアント相当: memory_space_id を載せない request
    let context = MemoryContext {
        cwd: Some(cwd.to_string_lossy().into_owned()),
        memory_space_id: None,
    };
    let response = service.apply(
        "a1".into(),
        "sess_legacy".into(),
        &context,
        goal_op("legacy style goal"),
    );
    match response {
        aibe_protocol::ClientResponse::MemoryApplyResult { entries, .. } => {
            assert_eq!(entries.len(), 1);
            // cwd から project-backed space に解決される
            assert!(entries[0].memory_space_id.starts_with("project_"));
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn now_is_stale_across_sessions_in_prompt_block() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c1 = ctx("sess_001", "ctx_a", &cwd);
    store.apply(&c1, &now_op("from sess 001"), 1).expect("now");
    let c2 = ctx("sess_002", "ctx_a", &cwd);
    let block = store
        .resolve_for_prompt(&c2, "query", MEMORY_PROMPT_BUDGET_BYTES)
        .expect("resolve");
    assert!(block.content.contains("stale"));
    assert!(block.content.contains("from sess 001"));
}

#[test]
fn sess_001_sets_goal_visible_from_sess_002_same_ctx_a() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c1 = ctx("sess_001", "ctx_a", &cwd);
    store
        .apply(&c1, &goal_op("ship memory split"), 1)
        .expect("apply");
    let c2 = ctx("sess_002", "ctx_a", &cwd);
    let entries = store
        .query(
            &c2,
            &MemoryQueryDto {
                kind: Some("goal".into()),
                scope: Some(MemoryScopeDto::Project),
                status: Some(MemoryStatusDto::Active),
                active_only: true,
                include_archived: false,
                limit: None,
                include_prompt_block: false,
                user_query: None,
            },
        )
        .expect("query");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].text, "ship memory split");
}

#[test]
fn sess_003_ctx_b_does_not_see_ctx_a_goal() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c1 = ctx("sess_001", "ctx_a", &cwd);
    store.apply(&c1, &goal_op("ctx a only"), 1).expect("apply");
    let c3 = ctx("sess_003", "ctx_b", &cwd);
    let entries = store
        .query(
            &c3,
            &MemoryQueryDto {
                kind: Some("goal".into()),
                scope: Some(MemoryScopeDto::Project),
                status: Some(MemoryStatusDto::Active),
                active_only: true,
                include_archived: false,
                limit: None,
                include_prompt_block: false,
                user_query: None,
            },
        )
        .expect("query");
    assert!(entries.is_empty());
}

#[test]
fn memory_query_include_prompt_block_returns_materialized_block() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c = ctx("sess", "ctx_a", &cwd);
    store.apply(&c, &goal_op("block goal"), 1).expect("goal");

    let resolver = aibe::adapters::outbound::FilesystemMemorySpaceResolver;
    let service = MemoryService::new(Arc::new(store), Arc::new(resolver));
    let response = service.query(
        "q1".into(),
        "sess".into(),
        &MemoryContext {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            memory_space_id: Some("ctx_a".into()),
        },
        MemoryQueryDto {
            kind: None,
            scope: None,
            status: None,
            active_only: false,
            include_archived: false,
            limit: None,
            include_prompt_block: true,
            user_query: None,
        },
    );
    match response {
        aibe_protocol::ClientResponse::MemoryQueryResult { prompt_block, .. } => {
            let block = prompt_block.expect("prompt block");
            assert!(block.contains("[aibe contextual memory]"));
            assert!(block.contains("block goal"));
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn memory_apply_session_scope_without_cwd_succeeds() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let resolver = aibe::adapters::outbound::FilesystemMemorySpaceResolver;
    let service = MemoryService::new(Arc::new(store), Arc::new(resolver));
    let response = service.apply(
        "a1".into(),
        "sess_legacy".into(),
        &MemoryContext {
            cwd: None,
            memory_space_id: Some("ctx_session".into()),
        },
        now_op("session now without cwd"),
    );
    match response {
        aibe_protocol::ClientResponse::MemoryApplyResult { entries, .. } => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].text, "session now without cwd");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn memory_apply_project_scope_without_cwd_is_invalid() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let resolver = aibe::adapters::outbound::FilesystemMemorySpaceResolver;
    let service = MemoryService::new(Arc::new(store), Arc::new(resolver));
    let response = service.apply(
        "a1".into(),
        "sess_legacy".into(),
        &MemoryContext {
            cwd: None,
            memory_space_id: Some("ctx_a".into()),
        },
        goal_op("needs cwd"),
    );
    match response {
        aibe_protocol::ClientResponse::Error { message, .. } => {
            assert!(message.contains("cwd is required"));
        }
        other => panic!("expected invalid request: {other:?}"),
    }
}

#[test]
fn memory_query_project_scope_without_cwd_is_invalid() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let resolver = aibe::adapters::outbound::FilesystemMemorySpaceResolver;
    let service = MemoryService::new(Arc::new(store), Arc::new(resolver));
    let response = service.query(
        "q1".into(),
        "sess".into(),
        &MemoryContext {
            cwd: None,
            memory_space_id: Some("ctx_a".into()),
        },
        MemoryQueryDto {
            kind: Some("goal".into()),
            scope: Some(MemoryScopeDto::Project),
            status: None,
            active_only: false,
            include_archived: false,
            limit: None,
            include_prompt_block: false,
            user_query: None,
        },
    );
    match response {
        aibe_protocol::ClientResponse::Error { message, .. } => {
            assert!(message.contains("cwd is required"));
        }
        other => panic!("expected invalid request: {other:?}"),
    }
}

#[test]
fn unsafe_session_id_is_rejected() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let resolver = aibe::adapters::outbound::FilesystemMemorySpaceResolver;
    let service = MemoryService::new(Arc::new(store), Arc::new(resolver));
    let cwd = std::env::current_dir().expect("cwd");
    let response = service.apply(
        "a1".into(),
        "../escape".into(),
        &MemoryContext {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            memory_space_id: Some("ctx_a".into()),
        },
        goal_op("x"),
    );
    match response {
        aibe_protocol::ClientResponse::Error { message, .. } => {
            assert!(message.contains("invalid session_id"));
        }
        other => panic!("expected invalid session_id: {other:?}"),
    }
}

#[test]
fn mem_clear_unknown_kind_archives_open_entries() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c = ctx("sess", "ctx_a", &cwd);
    let add = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "note".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::Manual,
        status: MemoryStatusDto::Open,
        text: "open note".into(),
        make_active: false,
    });
    store.apply(&c, &add, 1).expect("add");
    let clear = MemoryOperationDto::ClearKind(MemoryOperationClearKind {
        kind: "note".into(),
        scope: MemoryScopeDto::Project,
    });
    store.apply(&c, &clear, 2).expect("clear");
    let entries = store
        .query(
            &c,
            &MemoryQueryDto {
                kind: Some("note".into()),
                scope: Some(MemoryScopeDto::Project),
                status: Some(MemoryStatusDto::Archived),
                active_only: false,
                include_archived: true,
                limit: None,
                include_prompt_block: false,
                user_query: None,
            },
        )
        .expect("query");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, MemoryStatus::Archived);
}

#[test]
fn long_prompt_block_keeps_footer_and_truncates_goal_body() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    let c = ctx("sess", "ctx_a", &cwd);
    let long = "x".repeat(MEMORY_PROMPT_BUDGET_BYTES - 226);
    store.apply(&c, &goal_op(&long), 1).expect("goal");
    store.apply(&c, &now_op("short now"), 2).expect("now");
    let block = store
        .resolve_for_prompt(&c, "query", MEMORY_PROMPT_BUDGET_BYTES)
        .expect("resolve");
    assert!(block.content.ends_with("[/aibe contextual memory]"));
    assert!(block.content.contains("... truncated ..."));
    assert!(block.content.contains("[goal]"));
    assert!(!block.content.contains("[now]"));
    assert!(block.content.len() <= MEMORY_PROMPT_BUDGET_BYTES);
}
