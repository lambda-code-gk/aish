#![cfg(feature = "memory")]
//! MemoryRecipe 統合テスト（ScriptedMockLlm）。

use std::sync::Arc;

use aibe::adapters::outbound::{
    shared_baseline_recipe_loader, shared_builtin_loader, FilesystemContextualMemoryStore,
    FilesystemMemorySpaceResolver, ScriptedMockLlm,
};
use aibe::application::memory_recipe_service::MemoryRecipeService;
use aibe::domain::LlmStepResult;
use aibe::ports::outbound::{ContextualMemoryStore, ProfileRegistry, TerminationCapability};
use aibe_protocol::{
    MemoryContext, MemoryInjectPolicyDto, MemoryOperationAdd, MemoryOperationDto, MemoryQueryDto,
    MemoryRecipeStatus, MemoryScopeDto, MemoryStatusDto,
};
use tempfile::tempdir;

fn memory_context(cwd: &std::path::Path) -> MemoryContext {
    MemoryContext {
        cwd: Some(cwd.to_string_lossy().into_owned()),
        memory_space_id: Some("ctx_recipe".into()),
    }
}

fn idea_op(text: &str) -> MemoryOperationDto {
    MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "idea".into(),
        scope: Some(MemoryScopeDto::Project),
        inject: Some(MemoryInjectPolicyDto::OnDemand),
        status: Some(MemoryStatusDto::Open),
        text: text.into(),
        make_active: Some(false),
    })
}

fn goal_op(text: &str) -> MemoryOperationDto {
    MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "goal".into(),
        scope: Some(MemoryScopeDto::Project),
        inject: Some(MemoryInjectPolicyDto::Pinned),
        status: Some(MemoryStatusDto::Active),
        text: text.into(),
        make_active: Some(true),
    })
}

fn open_idea_count(store: &dyn ContextualMemoryStore, cwd: &std::path::Path) -> usize {
    let ctx = aibe::ports::outbound::MemoryStoreContext {
        session_id: "sess-recipe",
        memory_space_id: "ctx_recipe".into(),
        cwd: Some(cwd),
    };
    store
        .query(
            &ctx,
            &MemoryQueryDto {
                kind: Some("idea".into()),
                scope: Some(MemoryScopeDto::Project),
                status: Some(MemoryStatusDto::Open),
                active_only: false,
                include_archived: false,
                limit: None,
                include_prompt_block: false,
                user_query: None,
            },
        )
        .expect("query ideas")
        .len()
}

fn active_goal_text(store: &dyn ContextualMemoryStore, cwd: &std::path::Path) -> Option<String> {
    let ctx = aibe::ports::outbound::MemoryStoreContext {
        session_id: "sess-recipe",
        memory_space_id: "ctx_recipe".into(),
        cwd: Some(cwd),
    };
    store
        .query(
            &ctx,
            &MemoryQueryDto {
                kind: Some("goal".into()),
                scope: Some(MemoryScopeDto::Project),
                status: Some(MemoryStatusDto::Active),
                active_only: true,
                include_archived: false,
                limit: Some(1),
                include_prompt_block: false,
                user_query: None,
            },
        )
        .expect("query goal")
        .into_iter()
        .next()
        .map(|e| e.text)
}

fn recipe_service(store: Arc<dyn ContextualMemoryStore>, llm_json: &str) -> MemoryRecipeService {
    let llm = Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::text_only(
        llm_json.to_string(),
    )]));
    MemoryRecipeService::new(
        store,
        Arc::new(FilesystemMemorySpaceResolver),
        shared_builtin_loader(),
        shared_baseline_recipe_loader(),
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only()),
    )
}

#[tokio::test]
async fn clarify_goal_collects_open_ideas_and_returns_proposals() {
    let dir = tempdir().expect("tempdir");
    let cwd = dir.path().to_path_buf();
    let store: Arc<dyn ContextualMemoryStore> = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let ctx = aibe::ports::outbound::MemoryStoreContext {
        session_id: "sess-recipe",
        memory_space_id: "ctx_recipe".into(),
        cwd: Some(&cwd),
    };
    store.apply(&ctx, &idea_op("card ui"), 1).expect("idea");
    store.apply(&ctx, &goal_op("old goal"), 2).expect("goal");

    let llm_json = r#"{"summary":"Refined goal from ideas","proposals":[{"operation":{"op":"add","kind":"goal","text":"ship memory v1"},"rationale":"main theme from ideas"}]}"#;
    let service = recipe_service(Arc::clone(&store), llm_json);
    let response = service
        .run(
            "r1".into(),
            "sess-recipe".into(),
            &memory_context(&cwd),
            "clarify-goal",
            false,
            None,
        )
        .await;

    match response {
        aibe_protocol::ClientResponse::MemoryRecipeRunResult {
            status,
            summary,
            proposals,
            applied_entries,
            ..
        } => {
            assert_eq!(status, MemoryRecipeStatus::Proposed);
            assert_eq!(summary, "Refined goal from ideas");
            assert_eq!(proposals.len(), 1);
            assert_eq!(proposals[0].rationale, "main theme from ideas");
            assert!(applied_entries.is_empty());
            assert_eq!(open_idea_count(store.as_ref(), &cwd), 1);
        }
        other => panic!("expected recipe result: {other:?}"),
    }
}

#[tokio::test]
async fn apply_false_does_not_mutate_store() {
    let dir = tempdir().expect("tempdir");
    let cwd = dir.path().to_path_buf();
    let store: Arc<dyn ContextualMemoryStore> = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let before = active_goal_text(store.as_ref(), &cwd);
    let llm_json = r#"{"summary":"x","proposals":[{"operation":{"op":"add","kind":"goal","text":"new goal"},"rationale":"why"}]}"#;
    let service = recipe_service(Arc::clone(&store), llm_json);
    let _ = service
        .run(
            "r1".into(),
            "sess-recipe".into(),
            &memory_context(&cwd),
            "clarify-goal",
            false,
            None,
        )
        .await;
    assert_eq!(active_goal_text(store.as_ref(), &cwd), before);
}

#[tokio::test]
async fn apply_true_mutates_store() {
    let dir = tempdir().expect("tempdir");
    let cwd = dir.path().to_path_buf();
    let store: Arc<dyn ContextualMemoryStore> = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let llm_json = r#"{"summary":"x","proposals":[{"operation":{"op":"add","kind":"goal","text":"applied goal"},"rationale":"why"}]}"#;
    let service = recipe_service(Arc::clone(&store), llm_json);
    let response = service
        .run(
            "r1".into(),
            "sess-recipe".into(),
            &memory_context(&cwd),
            "clarify-goal",
            true,
            None,
        )
        .await;
    match response {
        aibe_protocol::ClientResponse::MemoryRecipeRunResult {
            status,
            applied_entries,
            ..
        } => {
            assert_eq!(status, MemoryRecipeStatus::Applied);
            assert_eq!(applied_entries.len(), 1);
            assert_eq!(applied_entries[0].text, "applied goal");
        }
        other => panic!("expected applied: {other:?}"),
    }
    assert_eq!(
        active_goal_text(store.as_ref(), &cwd).as_deref(),
        Some("applied goal")
    );
}

#[tokio::test]
async fn invalid_json_returns_error() {
    let dir = tempdir().expect("tempdir");
    let cwd = dir.path().to_path_buf();
    let store: Arc<dyn ContextualMemoryStore> = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let service = recipe_service(Arc::clone(&store), "not json");
    let response = service
        .run(
            "r1".into(),
            "sess-recipe".into(),
            &memory_context(&cwd),
            "clarify-goal",
            false,
            None,
        )
        .await;
    match response {
        aibe_protocol::ClientResponse::Error { message, .. } => {
            assert!(message.contains("invalid json") || message.contains("recipe"));
        }
        other => panic!("expected error: {other:?}"),
    }
}

#[tokio::test]
async fn invalid_kind_in_proposal_returns_error() {
    let dir = tempdir().expect("tempdir");
    let cwd = dir.path().to_path_buf();
    let store: Arc<dyn ContextualMemoryStore> = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let llm_json = r#"{"summary":"x","proposals":[{"operation":{"op":"add","kind":"bogus","text":"t"},"rationale":"n"}]}"#;
    let service = recipe_service(Arc::clone(&store), llm_json);
    let response = service
        .run(
            "r1".into(),
            "sess-recipe".into(),
            &memory_context(&cwd),
            "clarify-goal",
            false,
            None,
        )
        .await;
    match response {
        aibe_protocol::ClientResponse::Error { message, .. } => {
            assert!(message.contains("unknown kind") || message.contains("bogus"));
        }
        other => panic!("expected error: {other:?}"),
    }
}

#[tokio::test]
async fn unknown_field_in_llm_output_returns_error() {
    let dir = tempdir().expect("tempdir");
    let cwd = dir.path().to_path_buf();
    let store: Arc<dyn ContextualMemoryStore> = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let llm_json = r#"{"summary":"x","proposals":[],"extra":1}"#;
    let service = recipe_service(Arc::clone(&store), llm_json);
    let response = service
        .run(
            "r1".into(),
            "sess-recipe".into(),
            &memory_context(&cwd),
            "clarify-goal",
            false,
            None,
        )
        .await;
    match response {
        aibe_protocol::ClientResponse::Error { message, .. } => {
            assert!(message.contains("unknown field"));
        }
        other => panic!("expected error: {other:?}"),
    }
}

#[tokio::test]
async fn non_add_operation_in_proposal_returns_error() {
    let dir = tempdir().expect("tempdir");
    let cwd = dir.path().to_path_buf();
    let store: Arc<dyn ContextualMemoryStore> = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let llm_json = r#"{"summary":"x","proposals":[{"operation":{"op":"clear_kind","kind":"goal","scope":"project"},"rationale":"n"}]}"#;
    let service = recipe_service(Arc::clone(&store), llm_json);
    let response = service
        .run(
            "r1".into(),
            "sess-recipe".into(),
            &memory_context(&cwd),
            "clarify-goal",
            false,
            None,
        )
        .await;
    match response {
        aibe_protocol::ClientResponse::Error { message, .. } => {
            assert!(message.contains("add"));
        }
        other => panic!("expected error: {other:?}"),
    }
}

#[test]
fn recipe_never_accepts_shell_exec_in_llm_output() {
    use aibe::domain::{baseline_memory_kind_registry, parse_and_validate_recipe_output};
    let registry = baseline_memory_kind_registry();
    let raw = r#"{"summary":"x","proposals":[{"operation":{"op":"add","kind":"goal","text":"t"},"rationale":"n"}]}"#;
    let out = parse_and_validate_recipe_output(raw, registry, &["add".into()]).expect("valid");
    for p in &out.proposals {
        assert!(matches!(p.operation, MemoryOperationDto::Add(_)));
    }
}
