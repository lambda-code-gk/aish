use std::sync::Arc;

use aibe::adapters::outbound::{
    FilesystemMemorySpaceResolver, FilesystemWorkStore, StaticCapabilityPolicy,
};
use aibe::application::WorkService;
use aibe::ports::outbound::WorkStore;
use aibe_protocol::{ClientResponse, MemoryContext, WorkOperationDto, WorkQueryResponseBody};

#[test]
fn work_query_returns_empty_snapshot_from_real_store() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(FilesystemWorkStore::new(root.path().to_path_buf())) as Arc<dyn WorkStore>;
    let service = WorkService::new(
        store,
        Arc::new(FilesystemMemorySpaceResolver),
        StaticCapabilityPolicy::local_full(),
    );
    let response = service.query(
        "work-query".into(),
        "session".into(),
        &MemoryContext {
            cwd: None,
            memory_space_id: Some("project_test".into()),
        },
    );
    match response {
        ClientResponse::WorkQueryResult(WorkQueryResponseBody { snapshot, .. }) => {
            assert_eq!(snapshot.revision, 0);
            assert!(snapshot.active_work_id.is_none());
            assert!(snapshot.works.is_empty());
        }
        other => panic!("expected work query result: {other:?}"),
    }
}

#[test]
fn work_apply_rejects_invalid_programmatic_operation() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(FilesystemWorkStore::new(root.path().to_path_buf())) as Arc<dyn WorkStore>;
    let service = WorkService::new(
        store,
        Arc::new(FilesystemMemorySpaceResolver),
        StaticCapabilityPolicy::local_full(),
    );
    let response = service.apply(
        "work-apply".into(),
        "session".into(),
        &MemoryContext {
            cwd: None,
            memory_space_id: Some("project_test".into()),
        },
        WorkOperationDto::Switch { work_id: 0 },
    );
    match response {
        ClientResponse::Error { message, .. } => assert_eq!(message, "invalid work operation"),
        other => panic!("expected invalid request: {other:?}"),
    }
}

fn pending() {
    panic!("pending 0052");
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_start_creates_active_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_start_pauses_previous_active_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_focus_updates_active_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_idea_adds_entry_to_active_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_note_adds_entry_to_active_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_decision_adds_entry_to_active_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_defer_succeeds_without_active_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_defer_keeps_active_work_and_stack_unchanged() {
    pending();
}

#[test]
#[ignore = "0052 phase 2 pending"]
fn work_switch_changes_active_work_atomically() {
    pending();
}

#[test]
#[ignore = "0052 phase 2 pending"]
fn work_finish_marks_active_done_and_unsets_active() {
    pending();
}

#[test]
#[ignore = "0052 phase 2 pending"]
fn work_mutations_requiring_active_fail_without_state_change() {
    pending();
}

#[test]
#[ignore = "0052 phase 2 pending"]
fn work_switch_rejects_missing_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 2 pending"]
fn work_switch_rejects_done_work() {
    pending();
}

#[test]
#[ignore = "0052 phase 2 pending"]
fn work_root_transitions_reject_non_empty_stack() {
    pending();
}

#[test]
#[ignore = "0052 phase 3 pending"]
fn work_push_stacks_parent_and_activates_child() {
    pending();
}

#[test]
#[ignore = "0052 phase 3 pending"]
fn work_nested_push_preserves_parent_chain() {
    pending();
}

#[test]
#[ignore = "0052 phase 3 pending"]
fn work_pop_finishes_child_and_restores_parent() {
    pending();
}

#[test]
#[ignore = "0052 phase 3 pending"]
fn work_pop_rejects_empty_stack_without_state_change() {
    pending();
}

#[test]
#[ignore = "0052 phase 3 pending"]
fn work_pop_does_not_merge_child_entries_into_parent() {
    pending();
}
