use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use ai::adapters::outbound::{
    allocate_runtime_handoff_path, AishHumanShellLauncher, HumanTaskFileStore,
    SystemHumanTaskTimeFormatter,
};
use ai::application::{
    HumanTaskCancel, HumanTaskCoordinator, HumanTaskParentInput, HumanTaskStatus,
};
use ai::domain::human_task_checkpoint::*;
use ai::ports::outbound::*;
use aibe_protocol::{
    HandoffExecutionOutcome, HumanTaskRequest, PostHandoffObservation, ShellLogRange,
};

fn task() -> HumanTaskRequest {
    HumanTaskRequest {
        objective: "review deployment".into(),
        reason: None,
        instructions: vec!["inspect".into()],
        completion_criteria: vec!["report".into()],
    }
}
fn observation(cwd: &Path) -> PostHandoffObservation {
    PostHandoffObservation {
        cwd_exists: true,
        cwd: cwd.display().to_string(),
        git_head: Some("abc".into()),
        git_branch: Some("main".into()),
        git_status: Some("clean".into()),
        shell_log_tail: Some("bounded".into()),
        shell_log_truncated: Some(false),
        observation_errors: vec![],
        human_task_evidence: None,
    }
}
fn checkpoint(state: HumanTaskWorkflowState, cwd: PathBuf) -> HumanTaskCheckpointV1 {
    let mut value = HumanTaskCheckpointV1 {
        version: 1,
        task_id: HumanTaskId::parse("ht-20260714-7f31c2").unwrap(),
        state,
        task: task(),
        parent: HumanTaskParentContext {
            ai_session_id: "s1".into(),
            conversation_id: "c1".into(),
            turn_id: "t1".into(),
            user_request: "please review".into(),
            original_cwd: cwd.clone(),
            llm_profile: "fast".into(),
        },
        created_at_ms: 10,
        updated_at_ms: 10,
        suspended_at_ms: None,
        suspend_reason: None,
        current_cwd: cwd.clone(),
        segments: vec![],
        final_result: None,
        continuation: HumanTaskContinuationState::default(),
    };
    if state == HumanTaskWorkflowState::Suspended {
        value.updated_at_ms = 20;
        value.suspended_at_ms = Some(20);
        value.suspend_reason = Some("need approval".into());
        value.segments.push(HumanShellSegment {
            index: 0,
            shell_session_id: "shell-1".into(),
            started_at_ms: 10,
            ended_at_ms: 20,
            initial_cwd: cwd.clone(),
            final_cwd: cwd.clone(),
            shell_log_range: ShellLogRange {
                start: 2,
                end: Some(9),
            },
            observation: observation(&cwd),
            end_reason: HumanShellSegmentEnd::Suspended,
        });
    }
    value
}

struct Identity;
impl HumanTaskIdentity for Identity {
    fn new_task_id(&self) -> HumanTaskId {
        HumanTaskId::parse("ht-20260714-7f31c2").unwrap()
    }
    fn now_ms(&self) -> u64 {
        20
    }
}
struct Observer;
impl EnvironmentObserver for Observer {
    fn observe(
        &self,
        cwd: &Path,
        _: u64,
        _: Option<u64>,
        _: Option<&Path>,
    ) -> PostHandoffObservation {
        observation(cwd)
    }
}
struct Launcher {
    log: Arc<Mutex<Vec<&'static str>>>,
    suspended: bool,
}
impl HumanShellLauncher for Launcher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
        _: &AtomicBool,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        self.log.lock().unwrap().push("launch");
        let returned = HumanShellReturn {
            outcome: if self.suspended {
                HumanShellOutcome::Suspended
            } else {
                HumanShellOutcome::Done
            },
            suspend_reason: self.suspended.then(|| "need approval".into()),
            exit_code: Some(0),
            final_cwd: request.cwd.clone(),
            shell_session_id: "shell-1".into(),
            shell_session_dir: PathBuf::new(),
            shell_log_start: 2,
            shell_log_end: 9,
        };
        if self.suspended {
            Err(HumanShellLaunchError::Suspended {
                returned: Box::new(returned),
                reason: Some("need approval".into()),
            })
        } else {
            Ok(returned)
        }
    }
}
struct RecordingStore {
    log: Arc<Mutex<Vec<&'static str>>>,
    value: Mutex<Option<HumanTaskCheckpointV1>>,
    fail_save: bool,
}
struct NoopStoreLock;
impl HumanTaskStoreLock for NoopStoreLock {}

impl HumanTaskStore for RecordingStore {
    fn lock_exclusive(&self) -> Result<Box<dyn HumanTaskStoreLock + '_>, HumanTaskStoreError> {
        self.log.lock().unwrap().push("lock");
        Ok(Box::new(NoopStoreLock))
    }

    fn try_lock_exclusive(
        &self,
    ) -> Result<Option<Box<dyn HumanTaskStoreLock + '_>>, HumanTaskStoreError> {
        Ok(Some(self.lock_exclusive()?))
    }

    fn load_active(&self) -> Result<HumanTaskCheckpointV1, HumanTaskStoreError> {
        self.log.lock().unwrap().push("load");
        self.value
            .lock()
            .unwrap()
            .clone()
            .ok_or(HumanTaskStoreError::NotFound)
    }
    fn save(&self, value: &HumanTaskCheckpointV1) -> Result<(), HumanTaskStoreError> {
        self.log.lock().unwrap().push("save");
        if self.fail_save {
            Err(HumanTaskStoreError::Unavailable)
        } else {
            *self.value.lock().unwrap() = Some(value.clone());
            Ok(())
        }
    }
    fn remove(&self, _: &HumanTaskId) -> Result<(), HumanTaskStoreError> {
        self.log.lock().unwrap().push("remove");
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}
fn parent(cwd: &Path) -> HumanTaskParentInput {
    HumanTaskParentInput {
        ai_session_id: "s1".into(),
        conversation_id: "c1".into(),
        turn_id: "t1".into(),
        user_request: "please review".into(),
        cwd: cwd.into(),
        llm_profile: "fast".into(),
        runtime_dir: cwd.join("runtime"),
    }
}

#[test]
fn human_task_checkpoint_is_saved_before_shell_launch() {
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(vec![]));
    let store = RecordingStore {
        log: log.clone(),
        value: Mutex::new(None),
        fail_save: false,
    };
    let launcher = Launcher {
        log: log.clone(),
        suspended: true,
    };
    let result = HumanTaskCoordinator::new(&store, &Identity, &launcher, &Observer).execute(
        task(),
        parent(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Suspended);
    assert_eq!(
        &*log.lock().unwrap(),
        &["lock", "load", "save", "launch", "save"]
    );
    let failed_log = Arc::new(Mutex::new(vec![]));
    let failed = RecordingStore {
        log: failed_log.clone(),
        value: Mutex::new(None),
        fail_save: true,
    };
    let failed_launcher = Launcher {
        log: failed_log.clone(),
        suspended: false,
    };
    let result = HumanTaskCoordinator::new(&failed, &Identity, &failed_launcher, &Observer)
        .execute(task(), parent(dir.path()), &AtomicBool::new(false));
    assert_eq!(
        result.error.unwrap().code,
        "human_task_checkpoint_unavailable"
    );
    assert!(!failed_log.lock().unwrap().contains(&"launch"));

    let runtime_candidate = allocate_runtime_handoff_path();
    assert!(
        !runtime_candidate.exists(),
        "production runtime allocation must be side-effect free before checkpoint save"
    );
}

#[test]
fn human_task_checkpoint_v1_preserves_resume_context() {
    let value = checkpoint(
        HumanTaskWorkflowState::Suspended,
        PathBuf::from("/tmp/project"),
    );
    value.validate().unwrap();
    let json = serde_json::to_string(&value).unwrap();
    let decoded: HumanTaskCheckpointV1 = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(decoded.segments[0].index, 0);
    assert!(decoded.final_result.is_none());
    assert!(decoded.continuation.continuation_turn_id.is_none());
    for forbidden in [
        "api_key",
        "socket",
        "callback",
        "cancel_flag",
        "pty_fd",
        "environment",
    ] {
        assert!(!json.contains(forbidden));
    }
}

#[test]
fn human_task_checkpoint_store_is_secure_and_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let value = checkpoint(HumanTaskWorkflowState::Suspended, dir.path().into());
    store.save(&value).unwrap();
    let path = dir
        .path()
        .join("human-tasks/ht-20260714-7f31c2/checkpoint.json");
    assert_eq!(
        fs::metadata(dir.path().join("human-tasks"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let root_lock = store.lock_exclusive().unwrap();
    assert_eq!(
        fs::metadata(dir.path().join("human-tasks/lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(root_lock);
    assert_eq!(store.load_active().unwrap(), value);
    let mut changed = value.clone();
    changed.updated_at_ms = 21;
    changed.suspended_at_ms = Some(21);
    changed.segments[0].ended_at_ms = 21;
    store.save(&changed).unwrap();
    assert_eq!(store.load_active().unwrap(), changed);

    let oversized_dir = tempfile::tempdir().unwrap();
    let oversized_store = HumanTaskFileStore::new(oversized_dir.path().into());
    let mut oversized = checkpoint(
        HumanTaskWorkflowState::Suspended,
        oversized_dir.path().into(),
    );
    oversized.parent.user_request = "x".repeat(HUMAN_TASK_CHECKPOINT_MAX_BYTES);
    assert_eq!(
        oversized_store.save(&oversized).unwrap_err(),
        HumanTaskStoreError::Invalid
    );

    let symlink_dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), symlink_dir.path().join("human-tasks")).unwrap();
    assert_eq!(
        HumanTaskFileStore::new(symlink_dir.path().into())
            .save(&value)
            .unwrap_err(),
        HumanTaskStoreError::PermissionDenied
    );
}

#[test]
fn human_task_checkpoint_invalid_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("human-tasks/ht-20260714-7f31c2");
    fs::create_dir_all(&root).unwrap();
    fs::set_permissions(
        dir.path().join("human-tasks"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("checkpoint.json");
    let raw = b"{broken-json";
    fs::write(&path, raw).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    assert_eq!(
        store.load_active().unwrap_err(),
        HumanTaskStoreError::Invalid
    );
    assert_eq!(fs::read(&path).unwrap(), raw);
    assert!(HumanTaskCancel::new(&store).execute(|_| true).is_err());
    assert_eq!(fs::read(&path).unwrap(), raw);
}

#[test]
fn human_task_checkpoint_directory_without_checkpoint_is_invalid() {
    for residue in ["empty", "temp-only", "missing-checkpoint"] {
        let dir = tempfile::tempdir().unwrap();
        let task_dir = dir.path().join("human-tasks").join("ht-20260714-7f31c2");
        fs::create_dir_all(&task_dir).unwrap();
        fs::set_permissions(
            dir.path().join("human-tasks"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        fs::set_permissions(&task_dir, fs::Permissions::from_mode(0o700)).unwrap();
        match residue {
            "temp-only" => fs::write(task_dir.join(".checkpoint.123.tmp"), b"partial").unwrap(),
            "missing-checkpoint" => fs::write(task_dir.join("unexpected"), b"residue").unwrap(),
            _ => {}
        }

        let store = HumanTaskFileStore::new(dir.path().into());
        assert_eq!(
            store.load_active(),
            Err(HumanTaskStoreError::Invalid),
            "{residue} task directory must not be hidden as no active task"
        );
        assert!(matches!(
            HumanTaskCancel::new(&store).execute(|_| true),
            Err(ai::application::HumanTaskCancelError::Store(
                HumanTaskStoreError::Invalid
            ))
        ));
        assert!(task_dir.is_dir(), "invalid residue must be preserved");
    }

    let no_entry = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(no_entry.path().into());
    let lock = store.lock_exclusive().unwrap();
    drop(lock);
    assert_eq!(store.load_active(), Err(HumanTaskStoreError::NotFound));
}

#[test]
fn human_task_id_is_safe_path_component() {
    assert!(HumanTaskId::parse("ht-20260714-7f31c2").is_ok());
    for value in [
        "",
        ".",
        "..",
        "ht-20260714/7f31c2",
        "ht-20260714\\7f31c2",
        "ht-20260714-7F31C2",
        "ht-20260714-7f31c20",
        "ht-２０２６0714-7f31c2",
    ] {
        assert!(HumanTaskId::parse(value).is_err(), "{value}");
    }
}

#[test]
fn human_task_status_reports_suspended_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&checkpoint(
            HumanTaskWorkflowState::Suspended,
            dir.path().into(),
        ))
        .unwrap();
    let text = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    for expected in [
        "ht-20260714-7f31c2",
        "State: suspended",
        "review deployment",
        "need approval",
        "Current cwd:",
        "Resume:\n  ai human-task resume",
        "Cancel:\n  ai human-task cancel --yes",
    ] {
        assert!(text.contains(expected));
    }

    let escaped_dir = tempfile::tempdir().unwrap();
    let escaped_store = HumanTaskFileStore::new(escaped_dir.path().into());
    let mut unsafe_display =
        checkpoint(HumanTaskWorkflowState::Suspended, escaped_dir.path().into());
    unsafe_display.task.objective = "review\n\u{1b}[31msecret".into();
    escaped_store.save(&unsafe_display).unwrap();
    let escaped = HumanTaskStatus::new(&escaped_store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(escaped.contains(r"review\n\u{1b}[31msecret"));
    assert!(!escaped.contains('\u{1b}'));
}

#[test]
fn human_task_cancel_clears_suspended_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("history");
    let store = HumanTaskFileStore::new(history.clone());
    store
        .save(&checkpoint(
            HumanTaskWorkflowState::Suspended,
            dir.path().into(),
        ))
        .unwrap();
    let config = dir.path().join("ai.toml");
    fs::write(&config, format!("history_dir = {history:?}\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &config)
        .args(["human-task", "cancel", "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"Cancelled Human Task ht-20260714-7f31c2.\n");
    assert!(matches!(
        store.load_active(),
        Err(HumanTaskStoreError::NotFound)
    ));
    let no_task = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &config)
        .args(["human-task", "cancel", "--yes"])
        .output()
        .unwrap();
    assert!(no_task.status.success());
    assert_eq!(no_task.stdout, b"No suspended Human Task.\n");

    let log = Arc::new(Mutex::new(vec![]));
    let launcher = Launcher {
        log: log.clone(),
        suspended: false,
    };
    let result = HumanTaskCoordinator::new(&store, &Identity, &launcher, &Observer).execute(
        task(),
        parent(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Done);
    assert_eq!(&*log.lock().unwrap(), &["launch"]);
}

#[test]
fn human_task_orphaned_running_cancel_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("history");
    let store = HumanTaskFileStore::new(history.clone());
    store
        .save(&checkpoint(
            HumanTaskWorkflowState::Running,
            dir.path().into(),
        ))
        .unwrap();

    let status = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(status.contains("State: orphaned running"));
    assert!(status.contains("ai human-task cancel --yes"));

    let config = dir.path().join("ai.toml");
    fs::write(&config, format!("history_dir = {history:?}\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &config)
        .args(["human-task", "cancel", "--yes"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"Cancelled Human Task ht-20260714-7f31c2.\n");
    assert_eq!(store.load_active(), Err(HumanTaskStoreError::NotFound));

    let log = Arc::new(Mutex::new(vec![]));
    let launcher = Launcher {
        log: log.clone(),
        suspended: false,
    };
    let result = HumanTaskCoordinator::new(&store, &Identity, &launcher, &Observer).execute(
        task(),
        parent(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Done);
    assert_eq!(&*log.lock().unwrap(), &["launch"]);
}

#[test]
fn human_task_cancel_requires_confirmation_without_yes() {
    let dir = tempfile::tempdir().unwrap();
    let history = dir.path().join("history");
    let store = HumanTaskFileStore::new(history.clone());
    let active = checkpoint(HumanTaskWorkflowState::Suspended, dir.path().into());
    store.save(&active).unwrap();
    let before = serde_json::to_vec(&store.load_active().unwrap()).unwrap();
    let config = dir.path().join("ai.toml");
    fs::write(&config, format!("history_dir = {history:?}\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", config)
        .args(["human-task", "cancel"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "non-TTY stdin must not auto-confirm"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("human_task_cancel_not_confirmed"));
    assert_eq!(
        serde_json::to_vec(&store.load_active().unwrap()).unwrap(),
        before
    );

    assert!(matches!(
        HumanTaskCancel::new(&store).execute(|_| false),
        Err(ai::application::HumanTaskCancelError::NotConfirmed)
    ));
    assert!(store.load_active().is_ok());
}

struct RootLockCheckingLauncher {
    lock_path: PathBuf,
}

impl HumanShellLauncher for RootLockCheckingLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
        _: &AtomicBool,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        use std::os::unix::io::AsRawFd;

        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.lock_path)
            .unwrap();
        assert_ne!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0,
            "a second process must not acquire the root lock while Human Shell is active"
        );
        Ok(HumanShellReturn {
            outcome: HumanShellOutcome::Done,
            suspend_reason: None,
            exit_code: Some(0),
            final_cwd: request.cwd.clone(),
            shell_session_id: "shell-lock-check".into(),
            shell_session_dir: PathBuf::new(),
            shell_log_start: 0,
            shell_log_end: 0,
        })
    }
}

#[test]
fn human_task_create_holds_root_lock_until_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let launcher = RootLockCheckingLauncher {
        lock_path: dir.path().join("human-tasks/lock"),
    };
    let result = HumanTaskCoordinator::new(&store, &Identity, &launcher, &Observer).execute(
        task(),
        parent(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Done);
    let lock = store.lock_exclusive().unwrap();
    drop(lock);
}

#[test]
fn human_task_status_reports_active_running_without_blocking() {
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().to_path_buf();
    store
        .save(&checkpoint(HumanTaskWorkflowState::Running, cwd.clone()))
        .unwrap();
    let _held = store.lock_exclusive().unwrap();

    let started = Instant::now();
    let status = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "status must not block on flock while the owning Human Shell holds the root lock"
    );
    assert!(status.contains("State: running"));
    assert!(status.contains("This Human Shell session is active."));
    assert!(status.contains("human-task suspend"));
    assert!(!status.contains("orphaned"));
    assert!(!status.contains("ai human-task cancel"));

    assert!(matches!(
        HumanTaskCancel::new(&store).execute(|_| true),
        Err(ai::application::HumanTaskCancelError::Busy)
    ));
    assert!(store.load_active().is_ok());
}

#[test]
fn human_task_status_reports_no_task_as_success() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    assert_eq!(
        HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
            .render()
            .unwrap(),
        "No suspended Human Task.\n"
    );

    let config = dir.path().join("ai.toml");
    fs::write(
        &config,
        format!("history_dir = {:?}\n", dir.path().join("cli-history")),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", config)
        .args(["human-task", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"No suspended Human Task.\n");
}

#[test]
fn human_task_status_does_not_hide_invalid_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let task_dir = dir.path().join("human-tasks").join("ht-20260714-7f31c2");
    fs::create_dir_all(&task_dir).unwrap();
    fs::set_permissions(
        dir.path().join("human-tasks"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::set_permissions(&task_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let checkpoint_path = task_dir.join("checkpoint.json");
    fs::write(&checkpoint_path, b"{broken").unwrap();
    fs::set_permissions(&checkpoint_path, fs::Permissions::from_mode(0o600)).unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    assert!(HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .is_err());
    assert!(matches!(
        HumanTaskCancel::new(&store).execute(|_| true),
        Err(ai::application::HumanTaskCancelError::Store(
            HumanTaskStoreError::Invalid
        ))
    ));
    assert_eq!(fs::read(checkpoint_path).unwrap(), b"{broken");
}

#[test]
fn human_task_suspended_result_without_sidecar_preserves_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("fake-aish");
    let json = serde_json::json!({
        "outcome": "suspended",
        "exit_code": 0,
        "final_cwd": dir.path(),
        "shell_session_id": "shell-no-sidecar",
        "shell_session_dir": dir.path().join("session"),
        "shell_log_start": 0,
        "shell_log_end": 0
    });
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\nset -eu\nresult_file=\"$3\"\ncat > \"$result_file\" <<'JSON'\n{json}\nJSON\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();

    let store = HumanTaskFileStore::new(dir.path().join("history"));
    let launcher = AishHumanShellLauncher::new(fake);
    let result = HumanTaskCoordinator::new(&store, &Identity, &launcher, &Observer).execute(
        task(),
        parent(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Suspended);
    let saved = store.load_active().unwrap();
    assert_eq!(saved.state, HumanTaskWorkflowState::Suspended);
    assert_eq!(saved.suspend_reason, None);
}

#[test]
fn human_task_active_collision_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let active = checkpoint(HumanTaskWorkflowState::Suspended, dir.path().into());
    store.save(&active).unwrap();
    let before = serde_json::to_vec(&store.load_active().unwrap()).unwrap();
    let log = Arc::new(Mutex::new(vec![]));
    let launcher = Launcher {
        log: log.clone(),
        suspended: false,
    };
    let result = HumanTaskCoordinator::new(&store, &Identity, &launcher, &Observer).execute(
        task(),
        parent(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(result.error.unwrap().code, "human_task_already_active");
    assert!(log.lock().unwrap().is_empty());
    assert_eq!(
        serde_json::to_vec(&store.load_active().unwrap()).unwrap(),
        before
    );
}

#[test]
fn human_task_normal_done_leaves_no_suspend_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(Mutex::new(vec![]));
    let store = RecordingStore {
        log: log.clone(),
        value: Mutex::new(None),
        fail_save: false,
    };
    let launcher = Launcher {
        log,
        suspended: false,
    };
    let result = HumanTaskCoordinator::new(&store, &Identity, &launcher, &Observer).execute(
        task(),
        parent(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Done);
    assert!(!result.verified);
    assert!(matches!(
        store.load_active(),
        Err(HumanTaskStoreError::NotFound)
    ));
}
