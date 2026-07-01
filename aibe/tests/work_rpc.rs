use std::sync::Arc;

use aibe::adapters::outbound::{
    FilesystemMemorySpaceResolver, FilesystemWorkStore, StaticCapabilityPolicy,
};
use aibe::application::WorkService;
use aibe::domain::{WorkEntry, WorkEntryKind, WorkItem, WorkStatus};
use aibe::ports::outbound::{WorkStore, WorkStoreContext};
use aibe_protocol::{
    ClientResponse, MemoryContext, WorkApplyResponseBody, WorkEntryKindDto, WorkMutationOutcomeDto,
    WorkOperationDto, WorkQueryResponseBody, WorkSnapshotDto, WorkStatusDto,
};

struct Harness {
    _root: tempfile::TempDir,
    store: Arc<dyn WorkStore>,
    store_context: WorkStoreContext,
    service: WorkService,
    context: MemoryContext,
}

impl Harness {
    fn new(memory_space_id: &str) -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let store =
            Arc::new(FilesystemWorkStore::new(root.path().to_path_buf())) as Arc<dyn WorkStore>;
        let store_context = WorkStoreContext {
            memory_space_id: memory_space_id.into(),
        };
        Self {
            _root: root,
            service: WorkService::new(
                store.clone(),
                Arc::new(FilesystemMemorySpaceResolver),
                StaticCapabilityPolicy::local_full(),
            ),
            store,
            store_context,
            context: MemoryContext {
                cwd: None,
                memory_space_id: Some(memory_space_id.into()),
            },
        }
    }

    fn seed_stacked_state(&self) {
        self.store
            .mutate(&self.store_context, &mut |state| {
                state.next_work_id = 3;
                state.active_work_id = Some(2);
                state.stack = vec![1];
                state.works = vec![
                    work_item(1, WorkStatus::Paused, None),
                    work_item(2, WorkStatus::Active, Some(1)),
                ];
                Ok(())
            })
            .expect("seed stacked state");
    }

    fn seed_switchable_state(&self) {
        self.store
            .mutate(&self.store_context, &mut |state| {
                state.next_work_id = 4;
                state.active_work_id = Some(2);
                state.stack = Vec::new();
                state.works = vec![
                    work_item(1, WorkStatus::Paused, None),
                    work_item(2, WorkStatus::Active, None),
                    work_item(3, WorkStatus::Deferred, None),
                ];
                Ok(())
            })
            .expect("seed switchable state");
    }

    fn seed_finishable_state(&self) {
        self.store
            .mutate(&self.store_context, &mut |state| {
                state.next_work_id = 2;
                state.active_work_id = Some(1);
                state.stack = Vec::new();
                state.works = vec![work_item(1, WorkStatus::Active, None)];
                Ok(())
            })
            .expect("seed finishable state");
    }

    fn seed_pushable_state(&self) {
        self.store
            .mutate(&self.store_context, &mut |state| {
                state.next_work_id = 2;
                state.active_work_id = Some(1);
                state.stack = Vec::new();
                state.works = vec![work_item(1, WorkStatus::Active, None)];
                state.entries = vec![
                    WorkEntry {
                        id: 1,
                        work_id: 1,
                        kind: WorkEntryKind::Decision,
                        text: "root decision".into(),
                        created_at_ms: 1,
                    },
                    WorkEntry {
                        id: 2,
                        work_id: 1,
                        kind: WorkEntryKind::Note,
                        text: "root note".into(),
                        created_at_ms: 1,
                    },
                ];
                Ok(())
            })
            .expect("seed pushable state");
    }

    fn seed_nested_push_state(&self) {
        self.store
            .mutate(&self.store_context, &mut |state| {
                state.next_work_id = 3;
                state.active_work_id = Some(2);
                state.stack = vec![1];
                state.works = vec![
                    work_item(1, WorkStatus::Paused, None),
                    work_item(2, WorkStatus::Active, Some(1)),
                ];
                state.entries = vec![
                    WorkEntry {
                        id: 1,
                        work_id: 2,
                        kind: WorkEntryKind::Decision,
                        text: "child decision".into(),
                        created_at_ms: 1,
                    },
                    WorkEntry {
                        id: 2,
                        work_id: 2,
                        kind: WorkEntryKind::Idea,
                        text: "child idea".into(),
                        created_at_ms: 1,
                    },
                ];
                Ok(())
            })
            .expect("seed nested push state");
    }

    fn apply(&self, operation: WorkOperationDto) -> (WorkSnapshotDto, WorkMutationOutcomeDto) {
        match self.service.apply(
            "work-apply".into(),
            "session".into(),
            &self.context,
            operation,
        ) {
            ClientResponse::WorkApplyResult(WorkApplyResponseBody {
                snapshot, outcome, ..
            }) => (snapshot, outcome),
            other => panic!("expected work apply result: {other:?}"),
        }
    }

    fn query(&self) -> WorkSnapshotDto {
        match self
            .service
            .query("work-query".into(), "session".into(), &self.context)
        {
            ClientResponse::WorkQueryResult(WorkQueryResponseBody { snapshot, .. }) => snapshot,
            other => panic!("expected work query result: {other:?}"),
        }
    }
}

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
        WorkOperationDto::Focus { text: " ".into() },
    );
    match response {
        ClientResponse::Error { message, .. } => assert_eq!(message, "invalid work operation"),
        other => panic!("expected invalid request: {other:?}"),
    }
}

#[test]
fn work_start_creates_active_work() {
    let harness = Harness::new("project_start");
    let (snapshot, outcome) = harness.apply(WorkOperationDto::Start {
        goal: "ship phase one".into(),
    });
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.active_work_id, Some(1));
    assert_eq!(outcome.work_id, Some(1));
    assert_eq!(outcome.previous_work_id, None);
    let active = &snapshot.works[0];
    assert_eq!(active.goal, "ship phase one");
    assert_eq!(active.status, WorkStatusDto::Active);
    assert!(active.created_at_ms > 0);
    assert_eq!(active.created_at_ms, active.updated_at_ms);
}

#[test]
fn work_start_pauses_previous_active_work() {
    let harness = Harness::new("project_start_twice");
    harness.apply(WorkOperationDto::Start {
        goal: "first".into(),
    });
    let (snapshot, outcome) = harness.apply(WorkOperationDto::Start {
        goal: "second".into(),
    });
    assert_eq!(snapshot.active_work_id, Some(2));
    assert_eq!(outcome.previous_work_id, Some(1));
    assert_eq!(snapshot.works[0].status, WorkStatusDto::Paused);
    assert_eq!(snapshot.works[1].status, WorkStatusDto::Active);
    assert_eq!(snapshot.works[1].goal, "second");
}

#[test]
fn work_focus_updates_active_work() {
    let harness = Harness::new("project_focus");
    harness.apply(WorkOperationDto::Start {
        goal: "focus work".into(),
    });
    let (snapshot, outcome) = harness.apply(WorkOperationDto::Focus {
        text: "current concern".into(),
    });
    assert_eq!(outcome.work_id, snapshot.active_work_id);
    let active = snapshot.works.iter().find(|work| work.id == 1).unwrap();
    assert_eq!(active.focus.as_deref(), Some("current concern"));
    assert!(active.updated_at_ms >= active.created_at_ms);
}

#[test]
fn work_idea_adds_entry_to_active_work() {
    assert_entry(WorkEntryKindDto::Idea, "an idea", "project_idea");
}

#[test]
fn work_note_adds_entry_to_active_work() {
    assert_entry(WorkEntryKindDto::Note, "a note", "project_note");
}

#[test]
fn work_decision_adds_entry_to_active_work() {
    assert_entry(WorkEntryKindDto::Decision, "a decision", "project_decision");
}

#[test]
fn work_defer_succeeds_without_active_work() {
    let harness = Harness::new("project_defer_empty");
    let (snapshot, outcome) = harness.apply(WorkOperationDto::Defer {
        text: "later task".into(),
    });
    assert_eq!(snapshot.active_work_id, None);
    assert_eq!(outcome.work_id, Some(1));
    assert_eq!(snapshot.works[0].status, WorkStatusDto::Deferred);
    assert_eq!(snapshot.works[0].goal, "later task");
}

#[test]
fn work_defer_keeps_active_work_and_stack_unchanged() {
    let harness = Harness::new("project_defer_active");
    harness.seed_stacked_state();
    let before = harness.query();
    let (after, _) = harness.apply(WorkOperationDto::Defer {
        text: "side idea".into(),
    });
    assert_eq!(after.active_work_id, before.active_work_id);
    assert_eq!(after.stack, before.stack);
    assert_eq!(&after.works[..before.works.len()], before.works.as_slice());
    assert_eq!(after.works[2].status, WorkStatusDto::Deferred);
}

#[test]
fn phase3_pop_without_active_work_fails_without_state_change() {
    let harness = Harness::new("project_phase1_unsupported");
    let before = harness.query();
    let response = harness.service.apply(
        "unsupported".into(),
        "session".into(),
        &harness.context,
        WorkOperationDto::Pop,
    );
    assert!(matches!(response, ClientResponse::Error { .. }));
    assert_eq!(harness.query(), before);
}

#[test]
fn phase1_active_operations_fail_without_changing_empty_state() {
    for (memory_space_id, operation) in [
        (
            "project_focus_without_active",
            WorkOperationDto::Focus {
                text: "next".into(),
            },
        ),
        (
            "project_entry_without_active",
            WorkOperationDto::AddEntry {
                kind: WorkEntryKindDto::Note,
                text: "observation".into(),
            },
        ),
    ] {
        let harness = Harness::new(memory_space_id);
        let before = harness.query();
        let response = harness.service.apply(
            "requires-active".into(),
            "session".into(),
            &harness.context,
            operation,
        );
        assert!(matches!(response, ClientResponse::Error { .. }));
        assert_eq!(harness.query(), before);
    }
}

fn assert_entry(kind: WorkEntryKindDto, text: &str, memory_space_id: &str) {
    let harness = Harness::new(memory_space_id);
    harness.apply(WorkOperationDto::Start {
        goal: "entry work".into(),
    });
    let (snapshot, outcome) = harness.apply(WorkOperationDto::AddEntry {
        kind,
        text: text.into(),
    });
    assert_eq!(outcome.work_id, Some(1));
    assert_eq!(snapshot.entries.len(), 1);
    let entry = &snapshot.entries[0];
    assert_eq!(entry.id, 1);
    assert_eq!(entry.work_id, 1);
    assert_eq!(entry.kind, kind);
    assert_eq!(entry.text, text);
    assert!(entry.created_at_ms > 0);
}

fn work_item(id: u64, status: WorkStatus, parent_id: Option<u64>) -> WorkItem {
    WorkItem {
        id,
        title: format!("work {id}"),
        goal: format!("work {id}"),
        status,
        parent_id,
        created_at_ms: id,
        updated_at_ms: id,
        finished_at_ms: None,
        focus: None,
        summary: None,
    }
}

fn pending() {
    panic!("pending 0052");
}

#[test]
fn work_switch_changes_active_work_atomically() {
    let harness = Harness::new("project_phase2_switch");
    harness.seed_switchable_state();
    let before = harness.query();
    let (after, outcome) = harness.apply(WorkOperationDto::Switch { work_id: 3 });
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(outcome.kind, aibe_protocol::WorkMutationKindDto::Switch);
    assert_eq!(outcome.work_id, Some(3));
    assert_eq!(outcome.previous_work_id, Some(2));
    assert_eq!(after.active_work_id, Some(3));
    assert_eq!(after.stack, before.stack);
    assert_eq!(after.works[0].status, WorkStatusDto::Paused);
    assert_eq!(after.works[1].status, WorkStatusDto::Paused);
    assert_eq!(after.works[2].status, WorkStatusDto::Active);
    assert_eq!(after.works[1].updated_at_ms, after.works[2].updated_at_ms);
}

#[test]
fn work_switch_to_current_active_is_idempotent() {
    let harness = Harness::new("project_phase2_switch_idempotent");
    harness.seed_switchable_state();
    let before = harness.query();
    let (after, outcome) = harness.apply(WorkOperationDto::Switch { work_id: 2 });
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.active_work_id, before.active_work_id);
    assert_eq!(after.stack, before.stack);
    assert_eq!(after.works, before.works);
    assert_eq!(after.entries, before.entries);
    assert_eq!(outcome.kind, aibe_protocol::WorkMutationKindDto::Switch);
    assert_eq!(outcome.work_id, Some(2));
    assert_eq!(outcome.previous_work_id, Some(2));
}

#[test]
fn work_finish_marks_active_done_and_unsets_active() {
    let harness = Harness::new("project_phase2_finish");
    harness.seed_finishable_state();
    let before = harness.query();
    let (after, outcome) = harness.apply(WorkOperationDto::Finish);
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(outcome.kind, aibe_protocol::WorkMutationKindDto::Finish);
    assert_eq!(outcome.work_id, Some(1));
    assert_eq!(outcome.previous_work_id, None);
    assert!(after.active_work_id.is_none());
    assert_eq!(after.works[0].status, WorkStatusDto::Done);
    assert!(after.works[0].finished_at_ms.is_some());
    assert_eq!(after.stack, before.stack);
}

#[test]
fn work_mutations_requiring_active_fail_without_state_change() {
    for (memory_space_id, operation) in [
        (
            "project_focus_without_active",
            WorkOperationDto::Focus {
                text: "next".into(),
            },
        ),
        (
            "project_entry_without_active",
            WorkOperationDto::AddEntry {
                kind: WorkEntryKindDto::Note,
                text: "observation".into(),
            },
        ),
        (
            "project_push_without_active",
            WorkOperationDto::Push {
                goal: "child".into(),
            },
        ),
        ("project_pop_without_active", WorkOperationDto::Pop),
        ("project_finish_without_active", WorkOperationDto::Finish),
    ] {
        let harness = Harness::new(memory_space_id);
        let before = harness.query();
        let response = harness.service.apply(
            "requires-active".into(),
            "session".into(),
            &harness.context,
            operation,
        );
        match response {
            ClientResponse::Error { message, .. } => {
                assert!(message.contains("no active work"), "{message}");
            }
            other => panic!("expected error: {other:?}"),
        }
        assert_eq!(harness.query(), before);
    }
}

#[test]
fn work_switch_rejects_missing_work() {
    let harness = Harness::new("project_phase2_missing");
    harness.seed_switchable_state();
    let before = harness.query();
    let response = harness.service.apply(
        "switch-missing".into(),
        "session".into(),
        &harness.context,
        WorkOperationDto::Switch { work_id: 99 },
    );
    match response {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains("work #99"), "{message}");
            assert!(message.contains("not found"), "{message}");
        }
        other => panic!("expected error: {other:?}"),
    }
    assert_eq!(harness.query(), before);
}

#[test]
fn work_switch_rejects_done_work() {
    let harness = Harness::new("project_phase2_done");
    harness
        .store
        .mutate(&harness.store_context, &mut |state| {
            state.next_work_id = 3;
            state.active_work_id = Some(2);
            state.stack = Vec::new();
            state.works = vec![
                work_item(1, WorkStatus::Done, None),
                work_item(2, WorkStatus::Active, None),
            ];
            Ok(())
        })
        .expect("seed done target state");
    let before = harness.query();
    let response = harness.service.apply(
        "switch-done".into(),
        "session".into(),
        &harness.context,
        WorkOperationDto::Switch { work_id: 1 },
    );
    match response {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains("already done"), "{message}");
        }
        other => panic!("expected error: {other:?}"),
    }
    assert_eq!(harness.query(), before);
}

#[test]
fn work_root_transitions_reject_non_empty_stack() {
    let harness = Harness::new("project_phase2_stack");
    harness.seed_stacked_state();
    let before = harness.query();
    for operation in [
        WorkOperationDto::Start {
            goal: "new root".into(),
        },
        WorkOperationDto::Switch { work_id: 1 },
        WorkOperationDto::Finish,
    ] {
        let response = harness.service.apply(
            "stacked-root".into(),
            "session".into(),
            &harness.context,
            operation,
        );
        match response {
            ClientResponse::Error { message, .. } => {
                assert!(message.contains("work stack is not empty"), "{message}");
            }
            other => panic!("expected error: {other:?}"),
        }
        assert_eq!(harness.query(), before);
    }
}

#[test]
fn work_push_stacks_parent_and_activates_child() {
    let harness = Harness::new("project_phase3_push");
    harness.seed_pushable_state();
    let before = harness.query();
    let (after, outcome) = harness.apply(WorkOperationDto::Push {
        goal: "child work".into(),
    });
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(outcome.kind, aibe_protocol::WorkMutationKindDto::Push);
    assert_eq!(outcome.work_id, Some(2));
    assert_eq!(outcome.previous_work_id, Some(1));
    assert_eq!(after.active_work_id, Some(2));
    assert_eq!(after.stack, vec![1]);
    assert_eq!(after.works[0].status, WorkStatusDto::Paused);
    assert_eq!(after.works[1].status, WorkStatusDto::Active);
    assert_eq!(after.works[1].parent_id, Some(1));
}

#[test]
fn work_nested_push_preserves_parent_chain() {
    let harness = Harness::new("project_phase3_nested_push");
    harness.seed_nested_push_state();
    let (after, outcome) = harness.apply(WorkOperationDto::Push {
        goal: "grandchild".into(),
    });
    assert_eq!(after.active_work_id, Some(3));
    assert_eq!(after.stack, vec![1, 2]);
    assert_eq!(outcome.previous_work_id, Some(2));
    assert_eq!(after.works[1].status, WorkStatusDto::Paused);
    assert_eq!(after.works[2].status, WorkStatusDto::Active);
    assert_eq!(after.works[2].parent_id, Some(2));
}

#[test]
fn work_pop_finishes_child_and_restores_parent() {
    let harness = Harness::new("project_phase3_pop");
    harness.seed_nested_push_state();
    let before = harness.query();
    let (after, outcome) = harness.apply(WorkOperationDto::Pop);
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(outcome.kind, aibe_protocol::WorkMutationKindDto::Pop);
    assert_eq!(outcome.work_id, Some(2));
    assert_eq!(outcome.previous_work_id, Some(1));
    assert_eq!(after.active_work_id, Some(1));
    assert_eq!(after.stack, Vec::<u64>::new());
    assert_eq!(after.works[0].status, WorkStatusDto::Active);
    assert_eq!(after.works[1].status, WorkStatusDto::Done);
    assert!(after.works[1].finished_at_ms.is_some());
    assert_eq!(after.entries, before.entries);
}

#[test]
fn work_pop_rejects_empty_stack_without_state_change() {
    let harness = Harness::new("project_phase3_empty_stack");
    harness.seed_finishable_state();
    let before = harness.query();
    let response = harness.service.apply(
        "pop-empty".into(),
        "session".into(),
        &harness.context,
        WorkOperationDto::Pop,
    );
    match response {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains("work stack is empty"), "{message}");
        }
        other => panic!("expected error: {other:?}"),
    }
    assert_eq!(harness.query(), before);
}

#[test]
fn work_pop_does_not_merge_child_entries_into_parent() {
    let harness = Harness::new("project_phase3_no_merge");
    harness.seed_nested_push_state();
    let before = harness.query();
    let (after, _) = harness.apply(WorkOperationDto::Pop);
    assert_eq!(after.entries, before.entries);
    assert!(after.entries.iter().all(|entry| entry.work_id == 2
        && matches!(
            entry.kind,
            aibe_protocol::WorkEntryKindDto::Decision | aibe_protocol::WorkEntryKindDto::Idea
        )));
}
