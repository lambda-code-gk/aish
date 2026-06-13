//! user-defined MemoryKindRegistry（kinds.toml）の統合テスト。

use std::io::Write;
use std::sync::Arc;

use aibe::adapters::outbound::{FilesystemContextualMemoryStore, FilesystemMemorySpaceResolver};
use aibe::application::memory_service::MemoryService;
use aibe_protocol::{
    ClientResponse, MemoryContext, MemoryOperationAdd, MemoryOperationDto, MemoryScopeDto,
    MemoryStatusDto,
};
use tempfile::TempDir;

fn write_kinds(path: &std::path::Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    let mut f = std::fs::File::create(path).expect("create");
    f.write_all(body.as_bytes()).expect("write");
}

fn memory_service(dir: &TempDir) -> MemoryService {
    let store = FilesystemContextualMemoryStore::new(dir.path().to_path_buf());
    let loader = store.registry_loader();
    MemoryService::new(
        Arc::new(store),
        Arc::new(FilesystemMemorySpaceResolver),
        loader,
    )
}

#[test]
fn kind_list_reflects_space_local_override() {
    let dir = TempDir::new().expect("tempdir");
    write_kinds(
        &dir.path().join("memory/spaces/ctx_team/kinds.toml"),
        r#"
[kinds.goal]
description = "space-specific goal"
"#,
    );
    let service = memory_service(&dir);
    let cwd = std::env::current_dir().expect("cwd");
    let res = service.kind_list(
        "kl1".into(),
        "sess-001".into(),
        &MemoryContext {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            memory_space_id: Some("ctx_team".into()),
        },
    );
    match res {
        ClientResponse::MemoryKindListResult { kinds, .. } => {
            let goal = kinds.iter().find(|k| k.id == "goal").expect("goal kind");
            assert_eq!(goal.description, "space-specific goal");
        }
        other => panic!("expected kind list: {other:?}"),
    }
}

#[test]
fn invalid_kinds_toml_fails_explicit_memory_apply() {
    let dir = TempDir::new().expect("tempdir");
    write_kinds(
        &dir.path().join("memory/kinds.toml"),
        r#"
[kinds.goal]
default_scope = "not_a_scope"
"#,
    );
    let service = memory_service(&dir);
    let cwd = std::env::current_dir().expect("cwd");
    let res = service.apply(
        "a1".into(),
        "sess-001".into(),
        &MemoryContext {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            memory_space_id: Some("ctx_a".into()),
        },
        MemoryOperationDto::Add(MemoryOperationAdd {
            kind: "goal".into(),
            scope: None,
            inject: None,
            status: None,
            text: "ship memory".into(),
            make_active: None,
        }),
    );
    match res {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains("kind registry"));
        }
        other => panic!("expected registry error: {other:?}"),
    }
}

#[test]
fn custom_kind_from_server_kinds_toml_supports_add_defaulting() {
    let dir = TempDir::new().expect("tempdir");
    write_kinds(
        &dir.path().join("memory/kinds.toml"),
        r#"
[kinds.checklist]
description = "チェックリスト"
default_scope = "project"
default_inject = "manual"
default_status = "open"
lifecycle = "open_archive"
cardinality = "multiple"
clear_from = "open"
clear_to = "archived"
aliases = ["checklist"]
"#,
    );
    let service = memory_service(&dir);
    let cwd = std::env::current_dir().expect("cwd");
    let res = service.apply(
        "a1".into(),
        "sess-001".into(),
        &MemoryContext {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            memory_space_id: Some("ctx_a".into()),
        },
        MemoryOperationDto::Add(MemoryOperationAdd {
            kind: "checklist".into(),
            scope: None,
            inject: None,
            status: None,
            text: "run verify".into(),
            make_active: None,
        }),
    );
    match res {
        ClientResponse::MemoryApplyResult { entries, .. } => {
            assert_eq!(entries[0].kind, "checklist");
            assert_eq!(entries[0].scope, MemoryScopeDto::Project);
            assert_eq!(entries[0].status, MemoryStatusDto::Open);
        }
        other => panic!("expected apply ok: {other:?}"),
    }
}

#[test]
fn memory_query_with_prompt_block_fails_on_invalid_kinds_toml() {
    let dir = TempDir::new().expect("tempdir");
    write_kinds(
        &dir.path().join("memory/kinds.toml"),
        r#"
[kinds.goal]
default_scope = "not_a_scope"
"#,
    );
    let service = memory_service(&dir);
    let cwd = std::env::current_dir().expect("cwd");
    let res = service.query(
        "q1".into(),
        "sess-001".into(),
        &MemoryContext {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            memory_space_id: Some("ctx_a".into()),
        },
        aibe_protocol::MemoryQueryDto {
            kind: None,
            scope: None,
            status: None,
            active_only: false,
            include_archived: false,
            limit: None,
            include_prompt_block: true,
            user_query: Some("fix rust".into()),
        },
    );
    match res {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains("kind registry") || message.contains("parse"));
        }
        other => panic!("expected registry error: {other:?}"),
    }
}
