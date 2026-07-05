//! 0055 Phase 6 — 縦切り統合テスト（実 store + 必要最小限の実 process / shell）。
#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use ai::adapters::outbound::{
    FileHandoffCandidatePublisher, FileSuggestedCommandRecallStore, FilesystemHandoffStore,
    SystemHandoffRuntime,
};
use ai::application::{
    CollaborativeExecutionContext, CollaborativeShellExecPolicy, ParentShellExecRequest,
    RecoveryOwner, ResumeReturnedParent, HANDOFF_ENV_KEYS,
};
use ai::domain::{
    ChildGoalAchievement, ChildGoalCloseState, ChildGoalMeta, Handoff, HandoffCheckpoint,
    HandoffLease, HandoffState, RequestedShellExec, HANDOFF_SCHEMA_VERSION,
};
use ai::ports::outbound::{
    CheckpointRepository, CollaborativeChildGoalService, EnvironmentObservation,
    EnvironmentObserver, HandoffRepository, HandoffRuntime, HandoffShellSessionStore,
    HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
    LeaseAcquireRequest, LeaseRepository, ShellSessionIssueRequest, SideRunLockRepository,
};

struct Observer;
impl EnvironmentObserver for Observer {
    fn observe(&self, cwd: &Path, _start: u64) -> EnvironmentObservation {
        EnvironmentObservation {
            cwd_exists: cwd.is_dir(),
            cwd: cwd.display().to_string(),
            git_head: Some("slice-head".into()),
            git_branch: Some("main".into()),
            git_status: Some(" M slice.rs".into()),
            shell_log_end: Some(12),
        }
    }
}

fn require_bash_and_aish() -> Option<PathBuf> {
    if !Path::new("/bin/bash").is_file() {
        return None;
    }
    let aish = resolve_aish_bin();
    if aish.is_file() {
        Some(aish)
    } else {
        None
    }
}

fn resolve_aish_bin() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_aish") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let workspace = PathBuf::from(dir).join("..");
        for profile in ["debug", "release"] {
            let candidate = workspace.join("target").join(profile).join("aish");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("aish")
}

fn test_path_with_ai() -> String {
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ai"))
        .parent()
        .expect("ai bin parent");
    format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn slice_request(
    cwd: PathBuf,
    run_id: &str,
    command: &str,
    args: &[&str],
) -> ParentShellExecRequest {
    ParentShellExecRequest {
        parent_task_id: "slice-task".into(),
        parent_conversation_id: "slice-conv".into(),
        parent_run_id: run_id.into(),
        parent_goal_id: Some("slice-goal".into()),
        parent_goal: "finish slice".into(),
        parent_request_summary: "vertical slice".into(),
        conversation_snapshot: "snapshot".into(),
        conversation_summary: "summary".into(),
        work_stage_and_plan: String::new(),
        memory_space_id: Some("project_slice".into()),
        command: command.into(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        cwd,
        tool_call_id: format!("tool-{run_id}"),
        shell_log_start: 0,
        suggestion_cache_path: PathBuf::from("/tmp/unused-suggestions.json"),
    }
}

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
        let parent_lease = self
            .wait_for_lease(&request.handoff_id, Duration::from_secs(3), |lease| {
                lease.owner_client_id.starts_with("ai-parent-")
            })
            .map_err(HumanShellLaunchError::Failed)?;
        *self.parent_lease_seen.lock().unwrap() = true;
        assert_eq!(parent_lease.handoff_id, request.handoff_id);

        let result_file = tempfile::Builder::new()
            .prefix("aish-slice-result-")
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
            .map_err(HumanShellLaunchError::Failed)?;
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

struct MockWorkClientForHandoff {
    active: Arc<Mutex<Option<u64>>>,
    ops: Arc<Mutex<Vec<String>>>,
}

impl ai::ports::outbound::WorkClient for MockWorkClientForHandoff {
    fn work_query(
        &self,
        _session_id: &str,
        _context: &aibe_protocol::MemoryContext,
    ) -> Result<aibe_protocol::ClientResponse, ai::ports::outbound::AgentError> {
        use aibe_protocol::{WorkQueryResponseBody, WorkSnapshotDto};
        Ok(aibe_protocol::ClientResponse::WorkQueryResult(
            WorkQueryResponseBody {
                id: "q".into(),
                snapshot: WorkSnapshotDto {
                    revision: 1,
                    active_work_id: *self.active.lock().unwrap(),
                    stack: vec![],
                    works: vec![],
                    entries: vec![],
                },
            },
        ))
    }

    fn work_apply(
        &self,
        _session_id: &str,
        _context: &aibe_protocol::MemoryContext,
        operation: aibe_protocol::WorkOperationDto,
    ) -> Result<aibe_protocol::ClientResponse, ai::ports::outbound::AgentError> {
        use aibe_protocol::{
            WorkApplyResponseBody, WorkMutationKindDto, WorkMutationOutcomeDto, WorkSnapshotDto,
        };
        let outcome = match operation {
            aibe_protocol::WorkOperationDto::Start { .. } => {
                self.ops.lock().unwrap().push("Start".into());
                *self.active.lock().unwrap() = Some(1);
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Start,
                    work_id: Some(1),
                    previous_work_id: None,
                }
            }
            aibe_protocol::WorkOperationDto::Push { .. } => {
                self.ops.lock().unwrap().push("Push".into());
                *self.active.lock().unwrap() = Some(2);
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Push,
                    work_id: Some(2),
                    previous_work_id: Some(1),
                }
            }
            aibe_protocol::WorkOperationDto::Pop => {
                self.ops.lock().unwrap().push("Pop".into());
                *self.active.lock().unwrap() = Some(1);
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Pop,
                    work_id: Some(1),
                    previous_work_id: Some(2),
                }
            }
            _ => panic!("unexpected work op"),
        };
        Ok(aibe_protocol::ClientResponse::WorkApplyResult(
            WorkApplyResponseBody {
                id: "a".into(),
                snapshot: WorkSnapshotDto {
                    revision: 1,
                    active_work_id: outcome.work_id,
                    stack: vec![],
                    works: vec![],
                    entries: vec![],
                },
                outcome,
            },
        ))
    }
}

struct TrackingChildGoalService {
    inner: ai::adapters::outbound::AibeCollaborativeChildGoalService<MockWorkClientForHandoff>,
    close_calls: Arc<Mutex<Vec<String>>>,
}

impl CollaborativeChildGoalService for TrackingChildGoalService {
    fn create_child_goal(
        &self,
        meta: &mut ChildGoalMeta,
        cwd: &Path,
        parent_goal: &str,
        handoff_reason: &str,
        requested_command: &str,
        human_request: &str,
    ) -> Result<(), ai::ports::outbound::CollaborativeChildGoalError> {
        self.inner.create_child_goal(
            meta,
            cwd,
            parent_goal,
            handoff_reason,
            requested_command,
            human_request,
        )
    }

    fn close_child_goal(
        &self,
        meta: &ChildGoalMeta,
        cwd: &Path,
        reason: ai::domain::ChildGoalCloseReason,
    ) -> Result<(), ai::ports::outbound::CollaborativeChildGoalError> {
        self.close_calls
            .lock()
            .unwrap()
            .push(format!("{reason:?}:{:?}", meta.work_id));
        self.inner.close_child_goal(meta, cwd, reason)
    }
}

fn persist_side_slice_fixture(store: &FilesystemHandoffStore, handoff_id: &str) {
    let handoff = Handoff {
        id: handoff_id.into(),
        schema_version: HANDOFF_SCHEMA_VERSION,
        parent_task_id: "slice-task".into(),
        parent_conversation_id: "slice-conv".into(),
        parent_run_id: "slice-run".into(),
        parent_goal_id: Some("slice-goal".into()),
        child_goal_id: "child-slice".into(),
        side_conversation_id: Some("side-conv".into()),
        state: HandoffState::HumanActive,
        initial_cwd: "/workspace".into(),
        final_shell_cwd: None,
        parent_request_summary: "slice side".into(),
        requested_shell_execs: vec![RequestedShellExec {
            command: "cargo".into(),
            args: vec!["test".into()],
            cwd: Some("/workspace".into()),
            tool_call_id: Some("tool".into()),
        }],
        pending_human_request: None,
        conversation_snapshot_ref: "snap".into(),
        conversation_summary: "summary".into(),
        checkpoint_ref: "checkpoint.json".into(),
        before_observation_ref: "before".into(),
        after_observation_ref: None,
        shell_log_start: 0,
        shell_log_end: None,
        shell_generation: 1,
        return_reason: None,
        human_shell_exit_code: None,
        resume_error: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    store.save_handoff(&handoff).unwrap();
    let checkpoint = HandoffCheckpoint {
        parent_task_id: handoff.parent_task_id.clone(),
        parent_conversation_id: handoff.parent_conversation_id.clone(),
        parent_run_id: handoff.parent_run_id.clone(),
        pending_shell_exec: handoff.requested_shell_execs[0].clone(),
        parent_goal: "slice goal".into(),
        child_goal: ChildGoalMeta {
            id: handoff.child_goal_id.clone(),
            handoff_id: handoff.id.clone(),
            parent_goal_id: handoff.parent_goal_id.clone(),
            work_id: None,
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        },
        conversation_snapshot: "snap".into(),
        conversation_summary: "summary".into(),
        cwd: "/workspace".into(),
        environment_metadata:
            r#"{"host_id":"slice-host","effective_uid":1000,"memory_space_id":"project_slice"}"#
                .into(),
        handoff_id: handoff.id.clone(),
        side_conversation_id: handoff.side_conversation_id.clone(),
        command_candidates: vec![],
        shell_log_start: 0,
        control_state: HandoffState::HumanActive,
        provider_metadata: None,
        tool_executions: vec![],
    };
    store.save_checkpoint(handoff_id, &checkpoint).unwrap();
    store
        .append_shell_session(
            handoff_id,
            &ShellSessionIssueRequest {
                generation: 1,
                token_plaintext: "secret-token".into(),
                now_ms: 1,
            },
        )
        .unwrap();
}

fn run_aish_recall_from_cache(cache_path: &Path) -> String {
    let home = tempfile::tempdir().unwrap();
    let result_file = home.path().join("result.json");
    let mut child = Command::new(resolve_aish_bin())
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .env("PATH", test_path_with_ai())
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_ID", "ho-slice")
        .env("AISH_HANDOFF_TOKEN", "opaque-test-token")
        .env("AISH_HANDOFF_CONTEXT_VERSION", "1")
        .env("AI_SUGGESTION_CACHE", cache_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    let input = format!("ai recall next\nexit\n");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("human shell hung")
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn slice_lease_transfer_real_shell() {
    let Some(_aish) = require_bash_and_aish() else {
        return;
    };

    let store_root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(store_root.path().to_path_buf());
    let launcher = RealHumanShellLauncher::new(store_root.path().to_path_buf());
    let runtime = SystemHandoffRuntime;
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &Observer,
        &ai::ports::outbound::NoopParentToolBarrier,
        &ai::ports::outbound::NoopHandoffCandidatePublisher,
        &ai::ports::outbound::NoopCollaborativeChildGoalService,
        &runtime,
    );

    let mut request = slice_request(
        cwd.path().to_path_buf(),
        "lease-slice",
        "echo",
        &["lease-transfer"],
    );
    request.suggestion_cache_path = cwd.path().join(".ai-suggestions.json");

    let result = policy.intercept(request).expect("handoff intercept");
    assert!(*launcher.parent_lease_seen.lock().unwrap());
    assert!(*launcher.human_shell_lease_seen.lock().unwrap());

    assert!(
        store.load_lease(&result.handoff_id).unwrap().is_none(),
        "human shell return must release lease"
    );

    let resume = ResumeReturnedParent::new(&store, &Observer, &runtime);
    let owner = RecoveryOwner {
        client_id: format!("ai-parent-{}", runtime.process_id()),
        process_id: runtime.process_id(),
        tty: None,
    };
    resume
        .prepare(&result.handoff_id, &owner)
        .expect("parent re-acquires lease after shell return");
    assert!(store.load_lease(&result.handoff_id).unwrap().is_some());

    resume.finish(&result.handoff_id, Ok(())).expect("finish");
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::Completed
    );
    assert!(store.load_lease(&result.handoff_id).unwrap().is_none());
}

#[test]
fn slice_work_push_pop_real_store_shell() {
    let Some(_aish) = require_bash_and_aish() else {
        return;
    };

    let store_root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(store_root.path().to_path_buf());
    let active = Arc::new(Mutex::new(None));
    let ops = Arc::new(Mutex::new(Vec::new()));
    let close_calls = Arc::new(Mutex::new(Vec::new()));
    let child_goal = TrackingChildGoalService {
        inner: ai::adapters::outbound::AibeCollaborativeChildGoalService::new(
            MockWorkClientForHandoff {
                active: active.clone(),
                ops: ops.clone(),
            },
            "slice-session".into(),
            "project_slice".into(),
        ),
        close_calls: close_calls.clone(),
    };
    let launcher = RealHumanShellLauncher::new(store_root.path().to_path_buf());
    let runtime = SystemHandoffRuntime;
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &Observer,
        &ai::ports::outbound::NoopParentToolBarrier,
        &ai::ports::outbound::NoopHandoffCandidatePublisher,
        &child_goal,
        &runtime,
    );

    let mut request = slice_request(
        cwd.path().to_path_buf(),
        "work-slice",
        "cargo",
        &["test", "-p", "ai"],
    );
    request.suggestion_cache_path = cwd.path().join(".ai-suggestions.json");
    let handoff_result = policy.intercept(request).expect("handoff with child work");

    let handoff_id = handoff_result.handoff_id;
    let checkpoint = store.load_checkpoint(&handoff_id).unwrap();
    assert_eq!(checkpoint.child_goal.work_id, Some(2));
    assert!(ops.lock().unwrap().contains(&"Push".to_string()));
    assert_eq!(
        checkpoint.child_goal.close_state,
        Some(ChildGoalCloseState::Completed)
    );
    assert!(ops.lock().unwrap().contains(&"Pop".to_string()));
    assert!(!close_calls.lock().unwrap().is_empty());
    assert_eq!(*active.lock().unwrap(), Some(1));
}

#[test]
fn slice_candidate_cache_store_to_real_shell() {
    let Some(_aish) = require_bash_and_aish() else {
        return;
    };

    let home = tempfile::tempdir().unwrap();
    let previous_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", home.path());

    struct FastLauncher;
    impl HumanShellLauncher for FastLauncher {
        fn launch_and_wait(
            &self,
            request: &HumanShellLaunchRequest,
        ) -> Result<HumanShellReturn, HumanShellLaunchError> {
            Ok(HumanShellReturn {
                normal_return: true,
                exit_code: Some(0),
                final_cwd: request.cwd.clone(),
                shell_session_id: "slice-session".into(),
                shell_session_dir: request.cwd.clone(),
                shell_log_start: 0,
                shell_log_end: 1,
            })
        }
    }

    let store_root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(store_root.path().to_path_buf());
    let publisher = FileHandoffCandidatePublisher::new(
        FileSuggestedCommandRecallStore::new(home.path().join("unused.json")),
        "slice-session".into(),
    );
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &FastLauncher,
        &Observer,
        &ai::ports::outbound::NoopParentToolBarrier,
        &publisher,
        &ai::ports::outbound::NoopCollaborativeChildGoalService,
        &SystemHandoffRuntime,
    );

    let result = policy
        .intercept(slice_request(
            cwd.path().to_path_buf(),
            "cache-slice",
            "cargo",
            &["test", "-p", "ai"],
        ))
        .expect("handoff publishes candidate");

    let cache_path = home
        .path()
        .join(".local/share/ai/suggestions")
        .join(format!("handoff-{}.json", result.handoff_id));
    assert!(
        cache_path.is_file(),
        "candidate cache must exist at {}",
        cache_path.display()
    );

    let stdout = run_aish_recall_from_cache(&cache_path);
    assert!(
        stdout.contains("cargo") && stdout.contains("test") && stdout.contains("ai"),
        "recall output must include candidate command, stdout={stdout}"
    );

    if let Some(value) = previous_home {
        std::env::set_var("HOME", value);
    } else {
        std::env::remove_var("HOME");
    }
}

#[test]
fn slice_side_run_exclusive_real_store() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(FilesystemHandoffStore::new(tmp.path().join("store")));
    persist_side_slice_fixture(&store, "handoff-slice");

    let request_a = LeaseAcquireRequest {
        owner_client_id: "side-a".into(),
        owner_process_id: 123,
        owner_tty: None,
        owner_host: "slice-host".into(),
        owner_uid: 1000,
        now_ms: 10_000,
        lease_timeout_ms: 120_000,
    };
    let request_b = LeaseAcquireRequest {
        owner_client_id: "side-b".into(),
        owner_process_id: 456,
        owner_tty: None,
        owner_host: "slice-host".into(),
        owner_uid: 1000,
        now_ms: 10_001,
        lease_timeout_ms: 120_000,
    };

    let store_a = Arc::clone(&store);
    let req_a = request_a.clone();
    let first =
        std::thread::spawn(move || store_a.try_acquire_side_run_lock("handoff-slice", &req_a));

    let store_b = Arc::clone(&store);
    let req_b = request_b.clone();
    let second = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        store_b.try_acquire_side_run_lock("handoff-slice", &req_b)
    });

    let first_result = first.join().unwrap();
    let second_result = second.join().unwrap();
    assert!(
        first_result.is_ok(),
        "first side-run lock acquire: {first_result:?}"
    );
    assert!(
        second_result.is_err(),
        "second concurrent acquire must fail: {second_result:?}"
    );
    assert!(store.load_side_run_lock("handoff-slice").unwrap().is_some());
}

#[test]
fn slice_parent_resume_real_store_work() {
    let Some(_aish) = require_bash_and_aish() else {
        return;
    };

    let store_root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(store_root.path().to_path_buf());
    let active = Arc::new(Mutex::new(None));
    let ops = Arc::new(Mutex::new(Vec::new()));
    let close_calls = Arc::new(Mutex::new(Vec::new()));
    let child_goal = TrackingChildGoalService {
        inner: ai::adapters::outbound::AibeCollaborativeChildGoalService::new(
            MockWorkClientForHandoff {
                active: active.clone(),
                ops: ops.clone(),
            },
            "slice-session".into(),
            "project_slice".into(),
        ),
        close_calls: close_calls.clone(),
    };
    let launcher = RealHumanShellLauncher::new(store_root.path().to_path_buf());
    let create_runtime = SystemHandoffRuntime;
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &Observer,
        &ai::ports::outbound::NoopParentToolBarrier,
        &ai::ports::outbound::NoopHandoffCandidatePublisher,
        &child_goal,
        &create_runtime,
    );
    let mut request = slice_request(cwd.path().to_path_buf(), "resume-slice", "cargo", &["test"]);
    request.suggestion_cache_path = cwd.path().join(".ai-suggestions.json");
    let result = policy.intercept(request).expect("handoff to Returned");
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::Returned
    );

    let resume_runtime = SystemHandoffRuntime;
    let resume = ResumeReturnedParent::new(&store, &Observer, &resume_runtime);
    resume
        .prepare(
            &result.handoff_id,
            &RecoveryOwner {
                client_id: format!("ai-parent-{}", resume_runtime.process_id()),
                process_id: resume_runtime.process_id(),
                tty: None,
            },
        )
        .expect("prepare parent resume");
    resume
        .finish(&result.handoff_id, Ok(()))
        .expect("finish parent resume");
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::Completed
    );
    let checkpoint = store.load_checkpoint(&result.handoff_id).unwrap();
    assert_eq!(
        checkpoint.child_goal.close_state,
        Some(ChildGoalCloseState::Completed)
    );
    assert_eq!(
        checkpoint_memory_space_id(&checkpoint).as_deref(),
        Some("project_slice")
    );
}

fn checkpoint_memory_space_id(checkpoint: &HandoffCheckpoint) -> Option<String> {
    ai::application::checkpoint_memory_space_id(checkpoint)
}

#[test]
#[ignore = "nightly: structured human action requires mock aibe + human shell ai subprocess"]
fn slice_structured_human_action_real_shell_ai() {
    // RED: WAITING 状態の handoff store → human shell 内 bare `ai` → side 再開 → stderr 整形。
    // 実装後 run-collaborative-nightly.sh で実行し pending=false に昇格する。
    let tmp = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(tmp.path().join("store"));
    let handoff = Handoff {
        id: "human-action-slice".into(),
        schema_version: HANDOFF_SCHEMA_VERSION,
        parent_task_id: "slice-task".into(),
        parent_conversation_id: "slice-conv".into(),
        parent_run_id: "slice-run".into(),
        parent_goal_id: None,
        child_goal_id: "child".into(),
        side_conversation_id: Some("side".into()),
        state: HandoffState::SideAgentWaitingForHuman,
        initial_cwd: "/tmp".into(),
        final_shell_cwd: None,
        parent_request_summary: "run tests".into(),
        requested_shell_execs: vec![],
        pending_human_request: Some("Run cargo test".into()),
        conversation_snapshot_ref: "snap".into(),
        conversation_summary: "summary".into(),
        checkpoint_ref: "checkpoint.json".into(),
        before_observation_ref: "before".into(),
        after_observation_ref: None,
        shell_log_start: 0,
        shell_log_end: None,
        shell_generation: 1,
        return_reason: None,
        human_shell_exit_code: None,
        resume_error: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    store.save_handoff(&handoff).unwrap();
    let checkpoint = HandoffCheckpoint {
        parent_task_id: handoff.parent_task_id.clone(),
        parent_conversation_id: handoff.parent_conversation_id.clone(),
        parent_run_id: handoff.parent_run_id.clone(),
        pending_shell_exec: RequestedShellExec {
            command: "cargo".into(),
            args: vec!["test".into()],
            cwd: Some("/tmp".into()),
            tool_call_id: Some("tool".into()),
        },
        parent_goal: "goal".into(),
        child_goal: ChildGoalMeta {
            id: "child".into(),
            handoff_id: handoff.id.clone(),
            parent_goal_id: None,
            work_id: Some(2),
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        },
        conversation_snapshot: "snap".into(),
        conversation_summary: "summary".into(),
        cwd: "/tmp".into(),
        environment_metadata: r#"{"host_id":"slice-host","effective_uid":1000}"#.into(),
        handoff_id: handoff.id.clone(),
        side_conversation_id: handoff.side_conversation_id.clone(),
        command_candidates: vec![],
        shell_log_start: 0,
        control_state: HandoffState::SideAgentWaitingForHuman,
        provider_metadata: None,
        tool_executions: vec![],
    };
    store.save_checkpoint(&handoff.id, &checkpoint).unwrap();
    store
        .append_shell_session(
            &handoff.id,
            &ShellSessionIssueRequest {
                generation: 1,
                token_plaintext: "secret-token".into(),
                now_ms: 1,
            },
        )
        .unwrap();

    let mut env_pairs = Vec::new();
    for key in HANDOFF_ENV_KEYS {
        env_pairs.push((key, "placeholder"));
    }
    assert_eq!(
        store.load_handoff(&handoff.id).unwrap().state,
        HandoffState::SideAgentWaitingForHuman
    );
    assert!(handoff.pending_human_request.is_some());
    let _ = env_pairs;
    panic!("nightly slice not implemented: spawn human shell + bare ai with mock aibe");
}

#[test]
#[ignore = "nightly: full collaborative vertical chain"]
fn slice_full_collaborative_pty_nightly() {
    // RED: lease → work → candidate → side-run → human action → parent resume の最小連結。
    panic!("nightly full slice not implemented");
}
