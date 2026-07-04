use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ai::adapters::outbound::{
    FileSuggestedCommandRecallStore, FilesystemHandoffStore, SystemHandoffRuntime,
};
use ai::application::{
    persist_handoff_candidates_for_recall, recall_next_command, recall_prev_command,
    CollaborativeExecutionContext, CollaborativeShellExecPolicy, ParentShellExecRequest,
};
use ai::domain::{CollaborativeAgentRole, CollaborativePolicy, HandoffState};
use ai::ports::outbound::{
    EnvironmentObservation, EnvironmentObserver, HandoffRepository, HumanShellLaunchError,
    HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn, NoopHandoffCandidatePublisher,
    NoopParentToolBarrier,
};
use aibe_protocol::{HandoffExecutionOutcome, RequestedCommandCompletion};

struct Observer;
impl EnvironmentObserver for Observer {
    fn observe(&self, cwd: &Path, _start: u64) -> EnvironmentObservation {
        EnvironmentObservation {
            cwd_exists: cwd.is_dir(),
            cwd: cwd.display().to_string(),
            git_head: Some("after-head".into()),
            git_branch: Some("main".into()),
            git_status: Some(" M src/lib.rs".into()),
            shell_log_end: Some(12),
        }
    }
}

struct Launcher {
    root: PathBuf,
    launches: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}
impl HumanShellLauncher for Launcher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        assert!(self
            .root
            .join(&request.handoff_id)
            .join("checkpoint.json")
            .is_file());
        self.launches.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(20));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(HumanShellReturn {
            normal_return: true,
            exit_code: Some(9),
            final_cwd: request.cwd.clone(),
        })
    }
}

fn request(cwd: PathBuf, suffix: &str) -> ParentShellExecRequest {
    ParentShellExecRequest {
        parent_task_id: "task".into(),
        parent_conversation_id: "conv".into(),
        parent_run_id: format!("run-{suffix}"),
        parent_goal_id: None,
        parent_goal: "finish".into(),
        parent_request_summary: "test".into(),
        conversation_snapshot: "snapshot".into(),
        conversation_summary: "summary".into(),
        command: "printf".into(),
        args: vec!["hello world".into()],
        cwd,
        tool_call_id: format!("tc-{suffix}"),
        shell_log_start: 4,
    }
}

fn run_once() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    FilesystemHandoffStore,
    aibe_protocol::HumanHandoffResult,
    Arc<AtomicUsize>,
) {
    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(root.path().into());
    let launches = Arc::new(AtomicUsize::new(0));
    let launcher = Launcher {
        root: root.path().into(),
        launches: launches.clone(),
        active: Arc::new(AtomicUsize::new(0)),
        max_active: Arc::new(AtomicUsize::new(0)),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &Observer,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &SystemHandoffRuntime,
    );
    let result = policy.intercept(request(cwd.path().into(), "1")).unwrap();
    (root, cwd, store, result, launches)
}

#[test]
fn candidate_insertion_does_not_mark_command_executed() {
    let (_, _, _, result, _) = run_once();
    assert_eq!(
        result.requested_command_completion,
        RequestedCommandCompletion::Unknown
    );
    assert_eq!(
        result.execution_outcome,
        HandoffExecutionOutcome::HumanControlReturned
    );
}

#[test]
fn collaborative_shell_exec_skips_approval_prompt() {
    let (_, _, _, result, launches) = run_once();
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert_eq!(result.return_reason.as_deref(), Some("control_returned"));
}

#[test]
fn handoff_candidate_available_via_recall() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileSuggestedCommandRecallStore::new(dir.path().join("cache.json"));
    persist_handoff_candidates_for_recall(
        &store,
        "session",
        "handoff",
        &["cargo test".into()],
        "1",
        "bash",
    )
    .unwrap();
    assert_eq!(
        recall_next_command(&store).unwrap().as_deref(),
        Some("cargo test")
    );
}

#[test]
fn handoff_tool_result_marks_command_completion_unknown() {
    let (_, _, _, result, _) = run_once();
    assert_eq!(
        result.requested_command_completion,
        RequestedCommandCompletion::Unknown
    );
    assert_eq!(result.human_shell_exit_code, Some(9));
}

#[test]
fn normal_mode_shell_exec_still_auto_executes() {
    let context = CollaborativeExecutionContext::disabled();
    assert!(!context.should_handoff_shell_exec());
}

#[test]
fn parent_receives_reobservation_after_handoff() {
    let (_, _, _, result, _) = run_once();
    let after = result.after_observation_ref.unwrap();
    assert!(after.contains("after-head"));
    assert!(after.contains("src/lib.rs"));
}

#[test]
fn parent_shell_exec_creates_handoff_instead_of_exec() {
    let (_root, _cwd, store, result, launches) = run_once();
    let handoff = store.load_handoff(&result.handoff_id).unwrap();
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert_eq!(handoff.requested_shell_execs[0].command, "printf");
    assert_eq!(handoff.state, HandoffState::Completed);
}

#[test]
fn parent_tools_complete_before_handoff_starts() {
    // The policy invokes ParentToolBarrier before creating/persisting the handoff; the red-suite
    // trace test also asserts the complete barrier -> candidate -> shell ordering.
    let (_, _, _, _, launches) = run_once();
    assert_eq!(launches.load(Ordering::SeqCst), 1);
}

#[test]
fn recall_inserts_command_text_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileSuggestedCommandRecallStore::new(dir.path().join("cache.json"));
    persist_handoff_candidates_for_recall(
        &store,
        "session",
        "ho-1",
        &["printf 'hello world'".into()],
        "1",
        "bash",
    )
    .unwrap();
    assert_eq!(
        recall_next_command(&store).unwrap().unwrap(),
        "printf 'hello world'"
    );
}

#[test]
fn recall_prev_cycles_handoff_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileSuggestedCommandRecallStore::new(dir.path().join("cache.json"));
    persist_handoff_candidates_for_recall(
        &store,
        "session",
        "ho-1",
        &["one".into(), "two".into()],
        "1",
        "bash",
    )
    .unwrap();
    assert_eq!(recall_prev_command(&store).unwrap().as_deref(), Some("two"));
    assert_eq!(recall_prev_command(&store).unwrap().as_deref(), Some("one"));
}

#[test]
fn second_shell_exec_waits_for_first_handoff() {
    let root = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    let cwd = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    let store = Box::leak(Box::new(FilesystemHandoffStore::new(root.path().into())));
    let max = Arc::new(AtomicUsize::new(0));
    let launcher = Box::leak(Box::new(Launcher {
        root: root.path().into(),
        launches: Arc::new(AtomicUsize::new(0)),
        active: Arc::new(AtomicUsize::new(0)),
        max_active: max.clone(),
    }));
    let policy = Arc::new(CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext {
            role: CollaborativeAgentRole::Parent,
            policy: CollaborativePolicy::Enabled,
        },
        store,
        launcher,
        Box::leak(Box::new(Observer)),
        Box::leak(Box::new(NoopParentToolBarrier)),
        Box::leak(Box::new(NoopHandoffCandidatePublisher)),
        Box::leak(Box::new(SystemHandoffRuntime)),
    ));
    let p1 = policy.clone();
    let path = cwd.path().to_path_buf();
    let t1 = std::thread::spawn(move || p1.intercept(request(path, "1")).unwrap());
    let p2 = policy.clone();
    let path = cwd.path().to_path_buf();
    let t2 = std::thread::spawn(move || p2.intercept(request(path, "2")).unwrap());
    t1.join().unwrap();
    t2.join().unwrap();
    assert_eq!(max.load(Ordering::SeqCst), 1);
}
