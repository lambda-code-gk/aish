//! 0065 acceptance tests. Pending phases remain ignored.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use ai::adapters::outbound::HumanTaskFileStore;
use ai::application::{
    build_human_task_continuation_message, HumanTaskContinuation, HumanTaskContinuationRequest,
    HumanTaskResume, HumanTaskStatus,
};
use ai::domain::human_task_checkpoint::*;
use ai::ports::outbound::*;
use aibe_protocol::{
    HandoffExecutionOutcome, HumanTaskRequest, HumanTaskResult, PostHandoffObservation,
    ShellLogRange,
};

fn observation(cwd: &Path) -> PostHandoffObservation {
    PostHandoffObservation {
        cwd_exists: true,
        cwd: cwd.display().to_string(),
        git_head: Some("abc123".into()),
        git_branch: Some("main".into()),
        git_status: Some("clean".into()),
        shell_log_tail: Some("verified output".into()),
        shell_log_truncated: Some(false),
        observation_errors: vec![],
        human_task_evidence: None,
    }
}

fn pending_checkpoint(cwd: PathBuf) -> HumanTaskCheckpointV1 {
    let task = HumanTaskRequest {
        objective: "review deployment".into(),
        reason: Some("human judgment".into()),
        instructions: vec!["inspect state".into()],
        completion_criteria: vec!["report evidence".into()],
    };
    let range = ShellLogRange {
        start: 10,
        end: Some(20),
    };
    let observed = observation(&cwd);
    HumanTaskCheckpointV1 {
        version: 1,
        task_id: HumanTaskId::parse("ht-20260714-7f31c2").unwrap(),
        state: HumanTaskWorkflowState::ResultPending,
        task: task.clone(),
        parent: HumanTaskParentContext {
            ai_session_id: "ai-session-1".into(),
            conversation_id: "conversation-1".into(),
            turn_id: "original-turn".into(),
            user_request: "deploy and verify the service".into(),
            original_cwd: cwd.clone(),
            llm_profile: "fast".into(),
        },
        created_at_ms: 10,
        updated_at_ms: 20,
        suspended_at_ms: None,
        suspend_reason: None,
        current_cwd: cwd.clone(),
        segments: vec![HumanShellSegment {
            index: 0,
            shell_session_id: "shell-done".into(),
            started_at_ms: 10,
            ended_at_ms: 20,
            initial_cwd: cwd.clone(),
            final_cwd: cwd.clone(),
            shell_log_range: range.clone(),
            observation: observed.clone(),
            end_reason: HumanShellSegmentEnd::Done,
        }],
        final_result: Some(HumanTaskResult {
            status: HandoffExecutionOutcome::Done,
            task,
            verified: false,
            human_shell_exit_code: Some(0),
            final_shell_cwd: Some(cwd.display().to_string()),
            shell_log_range: Some(range),
            observation: Some(observed),
            error: None,
            task_id: None,
            suspend_reason: None,
        }),
        continuation: HumanTaskContinuationState::default(),
    }
}

#[test]
fn human_task_continuation_vertical_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&pending_checkpoint(dir.path().to_path_buf()))
        .unwrap();
    let captured = Arc::new(Mutex::new(None));
    let captured_turn = Arc::clone(&captured);

    let output = HumanTaskContinuation::new(&store, 30)
        .execute(None, move |request| {
            *captured_turn.lock().unwrap() = Some(request.clone());
            Ok(())
        })
        .unwrap();

    assert!(output.contains("continuation finished"));
    assert!(matches!(
        store.load_active(),
        Err(HumanTaskStoreError::NotFound)
    ));
    let request = captured.lock().unwrap().clone().unwrap();
    assert_eq!(request.turn_id, "ht-20260714-7f31c2-continuation");
    assert!(request
        .message
        .starts_with("[Collaborative Mode continuation]"));
}

#[test]
fn human_task_continuation_message_is_unverified() {
    let dir = tempfile::tempdir().unwrap();
    let checkpoint = pending_checkpoint(dir.path().to_path_buf());
    let message = build_human_task_continuation_message(&checkpoint).unwrap();
    assert!(message.contains("deploy and verify the service"));
    assert!(message.contains("review deployment"));
    assert!(message.contains("\"verified\": false"));
    assert!(message.contains("result is unverified"));
    assert!(message.contains("Re-observe the environment"));
    assert!(message.contains("Verify the completion criteria"));
}

#[test]
fn human_task_continuation_preserves_parent_context() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&pending_checkpoint(dir.path().to_path_buf()))
        .unwrap();
    let captured = Arc::new(Mutex::new(None::<HumanTaskContinuationRequest>));
    let captured_turn = Arc::clone(&captured);
    HumanTaskContinuation::new(&store, 30)
        .execute(None, move |request| {
            *captured_turn.lock().unwrap() = Some(request.clone());
            Ok(())
        })
        .unwrap();
    let request = captured.lock().unwrap().clone().unwrap();
    assert_eq!(request.ai_session_id, "ai-session-1");
    assert_eq!(request.conversation_id, "conversation-1");
    assert_eq!(request.cwd, dir.path());
    assert_eq!(request.llm_profile, "fast");
}

#[test]
fn human_task_result_pending_resume_retries_without_shell() {
    struct NoShell;
    impl HumanShellLauncher for NoShell {
        fn launch_and_wait(
            &self,
            _: &HumanShellLaunchRequest,
            _: &AtomicBool,
        ) -> Result<HumanShellReturn, HumanShellLaunchError> {
            panic!("ResultPending must not launch Human Shell");
        }
    }
    struct NoIdentity;
    impl HumanTaskIdentity for NoIdentity {
        fn new_task_id(&self) -> HumanTaskId {
            panic!("not used")
        }
        fn now_ms(&self) -> u64 {
            panic!("not used")
        }
    }
    struct NoObserver;
    impl EnvironmentObserver for NoObserver {
        fn observe(
            &self,
            _: &Path,
            _: u64,
            _: Option<u64>,
            _: Option<&Path>,
        ) -> PostHandoffObservation {
            panic!("not used")
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&pending_checkpoint(dir.path().to_path_buf()))
        .unwrap();
    let output = HumanTaskResume::new(&store, &NoIdentity, &NoShell, &NoObserver)
        .execute(None, dir.path().join("unused"), &AtomicBool::new(false))
        .unwrap();
    assert!(output.contains("Continuing saved Human Task result"));
    assert_eq!(
        store.load_active().unwrap().state,
        HumanTaskWorkflowState::ResultPending
    );
}

// Phase 1: the persisted ID is the retry identity.
// It is assigned before Continuing is saved.
// A normal turn failure restores ResultPending.
// The restored checkpoint retains the same ID.
// A later attempt receives that retained ID.
// No new identity is generated for the retry.
#[test]
fn human_task_continuation_turn_id_is_stable() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&pending_checkpoint(dir.path().to_path_buf()))
        .unwrap();
    let first = Arc::new(Mutex::new(String::new()));
    let first_turn = Arc::clone(&first);
    assert!(HumanTaskContinuation::new(&store, 30)
        .execute(None, move |request| {
            *first_turn.lock().unwrap() = request.turn_id.clone();
            Err(())
        })
        .is_err());
    let saved = store.load_active().unwrap();
    assert_eq!(saved.state, HumanTaskWorkflowState::ResultPending);
    assert_eq!(
        saved.continuation.continuation_turn_id.as_deref(),
        Some(first.lock().unwrap().as_str())
    );
    let second = Arc::new(Mutex::new(String::new()));
    let second_turn = Arc::clone(&second);
    HumanTaskContinuation::new(&store, 40)
        .execute(None, move |request| {
            *second_turn.lock().unwrap() = request.turn_id.clone();
            Ok(())
        })
        .unwrap();
    assert_eq!(*first.lock().unwrap(), *second.lock().unwrap());
}

#[test]
fn human_task_continuation_state_invariants() {
    let dir = tempfile::tempdir().unwrap();
    let mut checkpoint = pending_checkpoint(dir.path().to_path_buf());
    assert!(checkpoint.validate().is_ok());
    checkpoint.continuation.continuation_turn_id = Some("continuation-1".into());
    assert!(checkpoint.validate().is_ok());
    checkpoint.state = HumanTaskWorkflowState::Continuing;
    assert!(checkpoint.validate().is_ok());
    checkpoint.state = HumanTaskWorkflowState::Finished;
    assert!(checkpoint.validate().is_ok());
    checkpoint.continuation.continuation_turn_id = None;
    assert!(checkpoint.validate().is_err());
}

#[test]
fn human_task_continuation_failure_keeps_result_pending() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&pending_checkpoint(dir.path().to_path_buf()))
        .unwrap();
    assert!(HumanTaskContinuation::new(&store, 30)
        .execute(None, |_| Err(()))
        .is_err());
    let saved = store.load_active().unwrap();
    assert_eq!(saved.state, HumanTaskWorkflowState::ResultPending);
    assert!(saved.continuation.continuation_turn_id.is_some());
}

#[test]
fn human_task_continuation_holds_root_lock() {
    use std::os::unix::io::AsRawFd;
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&pending_checkpoint(dir.path().to_path_buf()))
        .unwrap();
    let lock_path = dir.path().join("human-tasks/lock");
    HumanTaskContinuation::new(&store, 30)
        .execute(None, |_| {
            let lock = std::fs::File::open(&lock_path).unwrap();
            assert_eq!(
                unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                -1
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn human_task_continuation_finished_delete_is_fail_closed() {
    struct RemoveFailStore {
        inner: HumanTaskFileStore,
    }
    impl HumanTaskStore for RemoveFailStore {
        fn lock_exclusive(&self) -> Result<Box<dyn HumanTaskStoreLock + '_>, HumanTaskStoreError> {
            self.inner.lock_exclusive()
        }
        fn try_lock_exclusive(
            &self,
        ) -> Result<Option<Box<dyn HumanTaskStoreLock + '_>>, HumanTaskStoreError> {
            self.inner.try_lock_exclusive()
        }
        fn load_active(&self) -> Result<HumanTaskCheckpointV1, HumanTaskStoreError> {
            self.inner.load_active()
        }
        fn save(&self, checkpoint: &HumanTaskCheckpointV1) -> Result<(), HumanTaskStoreError> {
            self.inner.save(checkpoint)
        }
        fn remove(&self, _: &HumanTaskId) -> Result<(), HumanTaskStoreError> {
            Err(HumanTaskStoreError::Unavailable)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = RemoveFailStore {
        inner: HumanTaskFileStore::new(dir.path().into()),
    };
    store
        .save(&pending_checkpoint(dir.path().to_path_buf()))
        .unwrap();
    assert!(HumanTaskContinuation::new(&store, 30)
        .execute(None, |_| Ok(()))
        .is_err());
    assert_eq!(
        store.load_active().unwrap().state,
        HumanTaskWorkflowState::Finished
    );
}

#[test]
fn human_task_continuation_status_and_cli_guidance() {
    struct Time;
    impl HumanTaskTimeFormatter for Time {
        fn format_local(&self, _: u64) -> String {
            "now".into()
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let mut checkpoint = pending_checkpoint(dir.path().to_path_buf());
    store.save(&checkpoint).unwrap();
    let pending = HumanTaskStatus::new(&store, &Time).render().unwrap();
    assert!(pending.contains("ai human-task resume"));
    checkpoint.state = HumanTaskWorkflowState::Continuing;
    checkpoint.continuation.continuation_turn_id = Some("continuation-1".into());
    store.save(&checkpoint).unwrap();
    let continuing = HumanTaskStatus::new(&store, &Time).render().unwrap();
    assert!(continuing.contains("State: continuing"));
    checkpoint.state = HumanTaskWorkflowState::Finished;
    store.save(&checkpoint).unwrap();
    let finished = HumanTaskStatus::new(&store, &Time).render().unwrap();
    assert!(finished.contains("State: finished"));
}

#[test]
fn human_task_continuation_preserves_resume_regressions() {
    let dir = tempfile::tempdir().unwrap();
    let mut checkpoint = pending_checkpoint(dir.path().to_path_buf());
    checkpoint.state = HumanTaskWorkflowState::Running;
    checkpoint.segments.clear();
    checkpoint.final_result = None;
    assert!(checkpoint.validate().is_ok());

    checkpoint.state = HumanTaskWorkflowState::Suspended;
    checkpoint.suspended_at_ms = Some(30);
    checkpoint.suspend_reason = Some("pause".into());
    checkpoint.segments.push(HumanShellSegment {
        index: 0,
        shell_session_id: "shell-suspended".into(),
        started_at_ms: 20,
        ended_at_ms: 30,
        initial_cwd: dir.path().into(),
        final_cwd: dir.path().into(),
        shell_log_range: ShellLogRange {
            start: 1,
            end: Some(2),
        },
        observation: observation(dir.path()),
        end_reason: HumanShellSegmentEnd::Suspended,
    });
    assert!(checkpoint.validate().is_ok());
}
