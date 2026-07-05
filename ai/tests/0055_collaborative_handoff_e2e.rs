use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use ai::adapters::outbound::{
    FileSuggestedCommandRecallStore, FilesystemHandoffStore, SystemHandoffRuntime,
};
use ai::application::{
    persist_handoff_candidates_for_recall, recall_next_command, recall_prev_command,
    CollaborativeExecutionContext, CollaborativeShellExecPolicy, ParentShellExecRequest,
    RecoveryOwner, ResumeReturnedParent,
};
use ai::domain::{CollaborativeAgentRole, CollaborativePolicy, HandoffLease, HandoffState};
use ai::ports::outbound::{
    EnvironmentObservation, EnvironmentObserver, HandoffRepository, HandoffRuntime,
    HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
    LeaseRepository, NoopCollaborativeChildGoalService, NoopHandoffCandidatePublisher,
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
            shell_session_id: "test-session".into(),
            shell_session_dir: request.cwd.clone(),
            shell_log_start: 0,
            shell_log_end: 1,
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
        work_stage_and_plan: String::new(),
        memory_space_id: None,
        command: "printf".into(),
        args: vec!["hello world".into()],
        cwd,
        tool_call_id: format!("tc-{suffix}"),
        shell_log_start: 4,
        suggestion_cache_path: PathBuf::from("/tmp/test-suggestions.json"),
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
        &NoopCollaborativeChildGoalService,
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
    assert_eq!(handoff.state, HandoffState::Returned);
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
        Box::leak(Box::new(NoopCollaborativeChildGoalService)),
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

fn resolve_aish_bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_aish") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(deps_dir) = exe.parent() {
            let sibling = deps_dir.join("aish");
            if sibling.is_file() {
                return sibling;
            }
            if let Some(debug_dir) = deps_dir.parent() {
                let debug_bin = debug_dir.join("aish");
                if debug_bin.is_file() {
                    return debug_bin;
                }
            }
        }
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let workspace = PathBuf::from(manifest).join("..");
        for profile in ["debug", "release"] {
            let candidate = workspace.join("target").join(profile).join("aish");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("aish")
}

/// 実 `aish human-shell` PTY を起動し、Ctrl+D 返却から親 resume までを通す launcher。
struct RealHumanShellLauncher {
    aish_bin: PathBuf,
    store_root: PathBuf,
    parent_lease_seen: Arc<Mutex<bool>>,
    human_shell_lease_seen: Arc<Mutex<bool>>,
}

impl RealHumanShellLauncher {
    fn new(store_root: PathBuf) -> Self {
        Self {
            aish_bin: resolve_aish_bin(),
            store_root,
            parent_lease_seen: Arc::new(Mutex::new(false)),
            human_shell_lease_seen: Arc::new(Mutex::new(false)),
        }
    }

    fn lease_path(&self, handoff_id: &str) -> PathBuf {
        self.store_root.join(handoff_id).join("lease.json")
    }

    fn read_lease(&self, handoff_id: &str) -> Option<HandoffLease> {
        let path = self.lease_path(handoff_id);
        if !path.is_file() {
            return None;
        }
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(raw.trim()).ok()
    }

    fn wait_for_lease<F>(
        &self,
        handoff_id: &str,
        timeout: Duration,
        predicate: F,
    ) -> Result<HandoffLease, String>
    where
        F: Fn(&HandoffLease) -> bool,
    {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(lease) = self.read_lease(handoff_id) {
                if predicate(&lease) {
                    return Ok(lease);
                }
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out waiting for lease on {}",
                    self.lease_path(handoff_id).display()
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl HumanShellLauncher for RealHumanShellLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        if !Path::new("/bin/bash").is_file() {
            return Err(HumanShellLaunchError::Failed(
                "real human-shell E2E requires /bin/bash".into(),
            ));
        }
        let parent_lease = self
            .wait_for_lease(&request.handoff_id, Duration::from_secs(3), |lease| {
                lease.owner_client_id.starts_with("ai-parent-")
            })
            .map_err(|error| HumanShellLaunchError::Failed(error))?;
        *self.parent_lease_seen.lock().unwrap() = true;
        assert_eq!(parent_lease.handoff_id, request.handoff_id);

        let result_file = tempfile::Builder::new()
            .prefix("aish-handoff-e2e-result-")
            .tempfile()
            .map_err(|error| HumanShellLaunchError::Failed(error.to_string()))?;
        let result_path = result_file.path().to_path_buf();
        drop(result_file);

        let home = tempfile::tempdir()
            .map_err(|error| HumanShellLaunchError::Failed(error.to_string()))?;
        let mut child = Command::new(&self.aish_bin)
            .args(["human-shell", "--result-file"])
            .arg(&result_path)
            .current_dir(&request.cwd)
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .env("AISH_CONTROL_MODE", "human-shell")
            .env("AISH_HANDOFF_ID", &request.handoff_id)
            .env("AISH_HANDOFF_TOKEN", &request.token)
            .env(
                "AISH_HANDOFF_CONTEXT_VERSION",
                request.context_version.to_string(),
            )
            .env("AISH_HANDOFF_STORE_ROOT", &self.store_root)
            .env("AISH_HANDOFF_HEARTBEAT_INTERVAL_MS", "200")
            .env("AISH_HANDOFF_LEASE_TIMEOUT_MS", "120000")
            .env("AI_SUGGESTION_CACHE", &request.suggestion_cache_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| HumanShellLaunchError::Failed(error.to_string()))?;
        let human_shell_pid = child.id();

        let transferred = self
            .wait_for_lease(&request.handoff_id, Duration::from_secs(5), |lease| {
                lease.owner_client_id.starts_with("aish-human-shell-")
                    && lease.owner_process_id == human_shell_pid
            })
            .map_err(|error| HumanShellLaunchError::Failed(error))?;
        *self.human_shell_lease_seen.lock().unwrap() = true;
        assert!(transferred.lease_expires_at_ms > transferred.last_heartbeat_at_ms);

        std::thread::sleep(Duration::from_millis(250));
        child
            .stdin
            .take()
            .ok_or_else(|| HumanShellLaunchError::Failed("human shell stdin unavailable".into()))?
            .write_all(b"\x04")
            .map_err(|error| HumanShellLaunchError::Failed(error.to_string()))?;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        let output = rx
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| HumanShellLaunchError::Failed("human shell hung after Ctrl+D".into()))?
            .map_err(|error| HumanShellLaunchError::Failed(error.to_string()))?;
        if !output.status.success() {
            return Err(HumanShellLaunchError::Failed(format!(
                "human shell failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let raw = std::fs::read_to_string(&result_path)
            .map_err(|_| HumanShellLaunchError::MissingReturnMarker)?;
        let mut returned: HumanShellReturn = serde_json::from_str(raw.trim())
            .map_err(|error| HumanShellLaunchError::Failed(error.to_string()))?;
        if !returned.normal_return {
            return Err(HumanShellLaunchError::MissingReturnMarker);
        }
        if returned.exit_code.is_none() {
            returned.exit_code = output.status.code();
        }
        Ok(returned)
    }
}

#[test]
fn collaborative_handoff_real_pty_ctrl_d_parent_resume_flow() {
    if !Path::new("/bin/bash").is_file() {
        return;
    }
    let aish_bin = resolve_aish_bin();
    assert!(
        aish_bin.is_file(),
        "aish binary not found at {}",
        aish_bin.display()
    );

    let store_root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(store_root.path().to_path_buf());
    let launcher = RealHumanShellLauncher::new(store_root.path().to_path_buf());
    let runtime = SystemHandoffRuntime;
    let suggestion_cache = cwd.path().join(".ai-suggestions.json");
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &Observer,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &NoopCollaborativeChildGoalService,
        &runtime,
    );

    let mut request = request(cwd.path().to_path_buf(), "real-pty");
    request.suggestion_cache_path = suggestion_cache;

    let result = policy.intercept(request).expect("handoff intercept");
    assert_eq!(
        result.execution_outcome,
        HandoffExecutionOutcome::HumanControlReturned
    );
    assert!(*launcher.parent_lease_seen.lock().unwrap());
    assert!(*launcher.human_shell_lease_seen.lock().unwrap());

    let handoff = store.load_handoff(&result.handoff_id).unwrap();
    assert_eq!(handoff.state, HandoffState::Returned);
    assert!(
        store.load_lease(&result.handoff_id).unwrap().is_none(),
        "human shell return must release lease before parent resume"
    );

    let owner = RecoveryOwner {
        client_id: format!("ai-parent-{}", runtime.process_id()),
        process_id: runtime.process_id(),
        tty: None,
    };
    let resume = ResumeReturnedParent::new(&store, &Observer, &runtime);
    resume
        .prepare(&result.handoff_id, &owner)
        .expect("parent must re-acquire lease after real human shell return");
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::ResumingParent
    );
    assert!(store.load_lease(&result.handoff_id).unwrap().is_some());

    resume
        .finish(&result.handoff_id, Ok(()))
        .expect("parent resume finish");
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::Completed
    );
    assert!(
        store.load_lease(&result.handoff_id).unwrap().is_none(),
        "completed handoff must not retain lease"
    );
}
