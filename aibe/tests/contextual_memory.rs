//! contextual memory store と prompt 注入の統合テスト。

use aibe::adapters::outbound::FilesystemContextualMemoryStore;
use aibe::ports::outbound::ContextualMemoryStore;
use aibe_protocol::{
    MemoryInjectPolicyDto, MemoryOperationDto, MemoryQueryDto, MemoryScopeDto, MemoryStatusDto,
    MEMORY_PROMPT_BUDGET_BYTES,
};
use tempfile::tempdir;

fn goal_op(text: &str) -> MemoryOperationDto {
    MemoryOperationDto::Add {
        kind: "goal".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::Pinned,
        status: MemoryStatusDto::Active,
        text: text.into(),
        make_active: true,
    }
}

fn now_op(text: &str) -> MemoryOperationDto {
    MemoryOperationDto::Add {
        kind: "now".into(),
        scope: MemoryScopeDto::Session,
        inject: MemoryInjectPolicyDto::Pinned,
        status: MemoryStatusDto::Active,
        text: text.into(),
        make_active: true,
    }
}

#[test]
fn resolve_for_prompt_injects_goal_now_not_idea_on_normal_query() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    store
        .apply("sess", Some(&cwd), &goal_op("build memory"), 1)
        .expect("goal");
    store
        .apply("sess", Some(&cwd), &now_op("wire protocol"), 2)
        .expect("now");
    let idea = MemoryOperationDto::Add {
        kind: "idea".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::OnDemand,
        status: MemoryStatusDto::Open,
        text: "later idea".into(),
        make_active: false,
    };
    store.apply("sess", Some(&cwd), &idea, 3).expect("idea");

    let block = store
        .resolve_for_prompt(
            "sess",
            Some(&cwd),
            "fix rust error",
            MEMORY_PROMPT_BUDGET_BYTES,
        )
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
    let idea = MemoryOperationDto::Add {
        kind: "idea".into(),
        scope: MemoryScopeDto::Project,
        inject: MemoryInjectPolicyDto::OnDemand,
        status: MemoryStatusDto::Open,
        text: "card idea".into(),
        make_active: false,
    };
    store.apply("sess", Some(&cwd), &idea, 1).expect("idea");

    let block = store
        .resolve_for_prompt(
            "sess",
            Some(&cwd),
            "今あるideaからMVPを整理して",
            MEMORY_PROMPT_BUDGET_BYTES,
        )
        .expect("resolve");
    assert!(block.content.contains("card idea"));
}

#[test]
fn now_add_twice_inactivates_old() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    store
        .apply("sess", Some(&cwd), &now_op("first"), 1)
        .expect("apply");
    store
        .apply("sess", Some(&cwd), &now_op("second"), 2)
        .expect("apply");
    let entries = store
        .query(
            "sess",
            Some(&cwd),
            &MemoryQueryDto {
                kind: Some("now".into()),
                scope: Some(MemoryScopeDto::Session),
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
    assert_eq!(entries[0].text, "second");
}

#[test]
fn now_clear_inactivates_active() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    store
        .apply("sess", Some(&cwd), &now_op("focus"), 1)
        .expect("apply");
    store
        .apply(
            "sess",
            Some(&cwd),
            &MemoryOperationDto::ClearActive {
                kind: "now".into(),
                scope: MemoryScopeDto::Session,
            },
            2,
        )
        .expect("clear");
    let entries = store
        .query(
            "sess",
            Some(&cwd),
            &MemoryQueryDto {
                kind: Some("now".into()),
                scope: Some(MemoryScopeDto::Session),
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
    store
        .apply("sess", Some(&cwd), &goal_op("block goal"), 1)
        .expect("goal");

    let service = aibe::application::memory_service::MemoryService::new(std::sync::Arc::new(store));
    let response = service.query(
        "q1".into(),
        "sess".into(),
        &cwd.to_string_lossy(),
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
fn same_session_query_from_store_sees_shared_state() {
    let dir = tempdir().expect("tempdir");
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let cwd = std::env::current_dir().expect("cwd");
    store
        .apply("sess", Some(&cwd), &goal_op("shared goal"), 1)
        .expect("apply");

    let entries = store
        .query(
            "sess",
            Some(&cwd),
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
    assert_eq!(entries[0].text, "shared goal");
}
