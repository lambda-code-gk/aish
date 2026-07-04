use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ai::adapters::outbound::{strip_handoff_environment, FilesystemHandoffStore};
use ai::application::{
    CollaborativeShellEnvironment, RequestHumanAction, SideAgentDispatch, SideAgentError,
    SideAgentInvocation, StartOrResumeSideAgent, HANDOFF_ENV_KEYS,
};
use ai::domain::{
    ChildGoalAchievement, ChildGoalMeta, CollaborativeAgentRole, CollaborativePolicy, Handoff,
    HandoffCheckpoint, HandoffState, RequestedShellExec, HANDOFF_SCHEMA_VERSION,
};
use ai::ports::outbound::{
    CheckpointRepository, EnvironmentObservation, EnvironmentObserver, HandoffRepository,
    HandoffRuntime, HandoffShellSessionStore, LeaseAcquireRequest, LeaseRepository,
    ShellSessionIssueRequest,
};
use tempfile::TempDir;

struct Runtime {
    seq: AtomicU64,
    host: String,
    uid: u32,
}

impl Runtime {
    fn valid() -> Self {
        Self {
            seq: AtomicU64::new(0),
            host: "test-host".into(),
            uid: 1000,
        }
    }
}

impl HandoffRuntime for Runtime {
    fn now_ms(&self) -> u64 {
        10_000 + self.seq.load(Ordering::Relaxed)
    }
    fn unique_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.seq.fetch_add(1, Ordering::Relaxed))
    }
    fn secure_token(&self) -> Result<String, String> {
        Ok("unused".into())
    }
    fn host_id(&self) -> String {
        self.host.clone()
    }
    fn effective_uid(&self) -> u32 {
        self.uid
    }
}

struct Observer;
impl EnvironmentObserver for Observer {
    fn observe(&self, cwd: &Path, start: u64) -> EnvironmentObservation {
        EnvironmentObservation {
            cwd_exists: true,
            cwd: cwd.display().to_string(),
            git_head: Some("abc".into()),
            git_branch: Some("main".into()),
            git_status: Some("M file".into()),
            shell_log_end: Some(start + 42),
        }
    }
}

struct Fixture {
    _tmp: TempDir,
    store: FilesystemHandoffStore,
    runtime: Runtime,
    env: CollaborativeShellEnvironment,
}

impl Fixture {
    fn new(state: HandoffState) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemHandoffStore::new(tmp.path().join("store"));
        let runtime = Runtime::valid();
        persist_fixture(&store, state, "test-host", 1000);
        Self {
            _tmp: tmp,
            store,
            runtime,
            env: CollaborativeShellEnvironment {
                handoff_id: "handoff-test".into(),
                token: "secret-token".into(),
                generation: 1,
            },
        }
    }

    fn service(&self) -> StartOrResumeSideAgent<'_, FilesystemHandoffStore, Observer, Runtime> {
        StartOrResumeSideAgent::new(&self.store, &Observer, &self.runtime)
    }

    fn invocation(&self, bare: bool, note: Option<&str>) -> SideAgentInvocation {
        SideAgentInvocation {
            standalone: false,
            collaborative_requested: false,
            bare,
            user_note: note.map(str::to_string),
            client_id: "client-1".into(),
            process_id: 123,
            tty: Some("/dev/pts/1".into()),
            cwd: PathBuf::from("/workspace"),
        }
    }
}

fn persist_fixture(store: &FilesystemHandoffStore, state: HandoffState, host: &str, uid: u32) {
    let requested = RequestedShellExec {
        command: "cargo".into(),
        args: vec!["test".into()],
        cwd: Some("/workspace".into()),
        tool_call_id: Some("tool-1".into()),
    };
    let child_goal = ChildGoalMeta {
        id: "goal-child".into(),
        handoff_id: "handoff-test".into(),
        parent_goal_id: Some("goal-parent".into()),
        close_reason: None,
        achievement: ChildGoalAchievement::Unknown,
    };
    store
        .save_handoff(&Handoff {
            id: "handoff-test".into(),
            schema_version: HANDOFF_SCHEMA_VERSION,
            parent_task_id: "task-parent".into(),
            parent_conversation_id: "conversation-parent".into(),
            parent_run_id: "run-parent".into(),
            parent_goal_id: Some("goal-parent".into()),
            child_goal_id: child_goal.id.clone(),
            side_conversation_id: None,
            state,
            initial_cwd: "/workspace".into(),
            final_shell_cwd: None,
            parent_request_summary: "Implement the requested feature safely".into(),
            requested_shell_execs: vec![requested.clone()],
            pending_human_request: Some("Review and run cargo test".into()),
            conversation_snapshot_ref: "checkpoint.json#conversation_snapshot".into(),
            conversation_summary: "Parent found a failing acceptance test".into(),
            checkpoint_ref: "checkpoint.json".into(),
            before_observation_ref: "before".into(),
            after_observation_ref: None,
            shell_log_start: 100,
            shell_log_end: None,
            shell_generation: 1,
            return_reason: None,
            human_shell_exit_code: None,
            resume_error: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        })
        .unwrap();
    store
        .save_checkpoint(
            "handoff-test",
            &HandoffCheckpoint {
                parent_task_id: "task-parent".into(),
                parent_conversation_id: "conversation-parent".into(),
                parent_run_id: "run-parent".into(),
                pending_shell_exec: requested,
                parent_goal: "Ship Phase 3 with all tests green".into(),
                child_goal,
                conversation_snapshot: "recent parent turn and unresolved issue".into(),
                conversation_summary: "Parent found a failing acceptance test".into(),
                cwd: "/workspace".into(),
                environment_metadata: serde_json::json!({
                    "handoff_host_id": host,
                    "handoff_uid": uid,
                })
                .to_string(),
                handoff_id: "handoff-test".into(),
                side_conversation_id: None,
                command_candidates: vec![],
                shell_log_start: 100,
                control_state: state,
                provider_metadata: Some("mock".into()),
                tool_executions: Vec::new(),
            },
        )
        .unwrap();
    store
        .append_shell_session(
            "handoff-test",
            &ShellSessionIssueRequest {
                generation: 1,
                token_plaintext: "secret-token".into(),
                now_ms: 1,
            },
        )
        .unwrap();
    if matches!(
        state,
        HandoffState::HumanActive
            | HandoffState::SideAgentRunning
            | HandoffState::SideAgentWaitingForHuman
    ) {
        store
            .try_acquire_lease(
                "handoff-test",
                &LeaseAcquireRequest {
                    owner_client_id: "ai-parent-42".into(),
                    owner_process_id: 42,
                    owner_tty: None,
                    owner_host: host.into(),
                    owner_uid: uid,
                    now_ms: 1,
                    lease_timeout_ms: 120_000,
                },
            )
            .unwrap();
    }
}

fn run_turn(fixture: &Fixture, note: Option<&str>) -> ai::application::SideTurn {
    match fixture
        .service()
        .dispatch(Some(fixture.env.clone()), &fixture.invocation(false, note))
        .unwrap()
    {
        SideAgentDispatch::Run(turn) => turn,
        other => panic!("expected side run, got {other:?}"),
    }
}

fn status_home(state: HandoffState) -> (TempDir, PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(".local/share/aibe/handoffs");
    let store = FilesystemHandoffStore::new(root);
    persist_fixture(&store, state, "test-host", 1000);
    let socket = home.path().join("missing.sock");
    (home, socket)
}

fn status_json(home: &Path, socket: &Path) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .args(["status", "--format", "json", "--quiet", "--socket"])
        .arg(socket)
        .env("HOME", home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn ai_status_does_not_invoke_llm() {
    let (home, socket) = status_home(HandoffState::HumanActive);
    let report = status_json(home.path(), &socket);
    assert_eq!(report["socket_alive"], false);
    assert!(report.get("collaborative_handoff").is_some());
}

#[test]
fn ai_status_never_prints_handoff_token() {
    let (home, socket) = status_home(HandoffState::HumanActive);
    let output = status_json(home.path(), &socket).to_string();
    assert!(!output.contains("secret-token"));
    assert!(!output.contains("token_hash"));
}

#[test]
fn ai_status_shows_collaborative_handoff_fields() {
    let (home, socket) = status_home(HandoffState::SideAgentWaitingForHuman);
    let report = status_json(home.path(), &socket);
    let handoff = &report["collaborative_handoff"][0];
    assert_eq!(
        handoff["parent_task"],
        "Implement the requested feature safely"
    );
    assert_eq!(handoff["state"], "SIDE_AGENT_WAITING_FOR_HUMAN");
    assert!(handoff["resume_hint"]
        .as_str()
        .unwrap()
        .contains("resume side"));
}

#[test]
fn ai_status_unchanged_without_active_handoff() {
    let home = tempfile::tempdir().unwrap();
    let report = status_json(home.path(), &home.path().join("missing.sock"));
    assert!(report.get("collaborative_handoff").is_none());
}

#[test]
fn ai_with_note_sets_user_note_on_resume() {
    let fixture = Fixture::new(HandoffState::SideAgentWaitingForHuman);
    let turn = run_turn(&fixture, Some("tests now pass"));
    assert_eq!(
        turn.control_returned.unwrap().user_note.as_deref(),
        Some("tests now pass")
    );
}

#[test]
fn bare_ai_in_human_active_opens_input_ui() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(fixture.env.clone()), &fixture.invocation(true, None))
            .unwrap(),
        SideAgentDispatch::PromptForInput { .. }
    ));
}

#[test]
fn bare_ai_resumes_side_agent_from_waiting() {
    let fixture = Fixture::new(HandoffState::SideAgentWaitingForHuman);
    assert!(run_turn(&fixture, None).control_returned.is_some());
}

#[test]
fn conversation_summary_updates_on_side_turn() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    run_turn(&fixture, None);
    let started = fixture
        .store
        .load_handoff("handoff-test")
        .unwrap()
        .conversation_summary;
    assert!(started.contains("side run started"));
    fixture
        .service()
        .finish_side_turn("handoff-test", "side completed analysis")
        .unwrap();
    let finished = fixture
        .store
        .load_handoff("handoff-test")
        .unwrap()
        .conversation_summary;
    assert!(finished.contains("side completed analysis"));
}

#[test]
fn handoff_rejected_when_effective_uid_mismatches() {
    let mut fixture = Fixture::new(HandoffState::HumanActive);
    fixture.runtime.uid = 2000;
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(fixture.env.clone()), &fixture.invocation(false, None)),
        Err(SideAgentError::UidMismatch)
    ));

    fixture.runtime.uid = 1000;
    let mut checkpoint = fixture.store.load_checkpoint("handoff-test").unwrap();
    checkpoint.environment_metadata = "{}".into();
    fixture
        .store
        .save_checkpoint("handoff-test", &checkpoint)
        .unwrap();
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(fixture.env.clone()), &fixture.invocation(false, None)),
        Err(SideAgentError::InvalidIdentityMetadata)
    ));
}

#[test]
fn handoff_rejected_when_host_id_mismatches() {
    let mut fixture = Fixture::new(HandoffState::HumanActive);
    fixture.runtime.host = "another-host".into();
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(fixture.env.clone()), &fixture.invocation(false, None)),
        Err(SideAgentError::HostMismatch)
    ));
}

#[test]
fn handoff_token_not_in_llm_context() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let turn = run_turn(&fixture, None);
    assert!(!turn.system_instruction.contains("secret-token"));
    assert!(!serde_json::to_string(&turn)
        .unwrap()
        .contains("secret-token"));
}

#[test]
fn human_control_returned_includes_required_fields() {
    let fixture = Fixture::new(HandoffState::SideAgentWaitingForHuman);
    let mut invocation = fixture.invocation(false, Some("done"));
    invocation.cwd = PathBuf::from("/workspace/current");
    let event = match fixture
        .service()
        .dispatch(Some(fixture.env.clone()), &invocation)
        .unwrap()
    {
        SideAgentDispatch::Run(turn) => turn.control_returned.unwrap(),
        other => panic!("expected side run, got {other:?}"),
    };
    assert!(!event.pending_request.is_empty());
    assert_eq!(event.shell_log_delta, "100..142");
    assert_eq!(event.current_cwd, "/workspace/current");
    assert!(event.current_observation.contains("git_head"));
    assert_eq!(event.user_note.as_deref(), Some("done"));
}

#[test]
fn incomplete_handoff_env_shows_error_not_fallback() {
    let env = HashMap::from([("AISH_HANDOFF_ID".into(), "handoff-test".into())]);
    assert!(matches!(
        CollaborativeShellEnvironment::from_map(&env),
        Err(SideAgentError::IncompleteEnvironment)
    ));
}

#[test]
fn nested_collaborative_flag_is_rejected() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let mut invocation = fixture.invocation(false, None);
    invocation.collaborative_requested = true;
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(fixture.env.clone()), &invocation),
        Err(SideAgentError::NestedCollaborative)
    ));
}

#[test]
fn orphaned_handoff_direct_ai_shows_resume_hint() {
    let fixture = Fixture::new(HandoffState::Orphaned);
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(fixture.env.clone()), &fixture.invocation(false, None)),
        Err(SideAgentError::Orphaned)
    ));
}

#[test]
fn side_agent_cannot_spawn_nested_human_shell() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let turn = run_turn(&fixture, None);
    assert!(!turn.collaborative_handoff);
    assert!(turn
        .system_instruction
        .contains("Do not start a collaborative handoff"));
}

#[test]
fn side_agent_receives_parent_task_context() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let context = run_turn(&fixture, None).system_instruction;
    for expected in [
        "Ship Phase 3",
        "failing acceptance test",
        "cargo",
        "goal-child",
        "/workspace",
        "handoff-test",
    ] {
        assert!(context.contains(expected), "missing {expected}: {context}");
    }
}

#[test]
fn side_agent_reuses_conversation_in_handoff() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let first = run_turn(&fixture, None).conversation_id;
    fixture
        .service()
        .finish_side_turn("handoff-test", "first done")
        .unwrap();
    let mut next_process = fixture.invocation(false, None);
    next_process.client_id = "client-2".into();
    next_process.process_id = 456;
    let second = match fixture
        .service()
        .dispatch(Some(fixture.env.clone()), &next_process)
        .unwrap()
    {
        SideAgentDispatch::Run(turn) => turn.conversation_id,
        other => panic!("expected side run, got {other:?}"),
    };
    assert_eq!(first, second);
}

#[test]
fn side_run_does_not_replace_or_release_parent_lifetime_lease() {
    let fixture = Fixture::new(HandoffState::HumanActive);

    run_turn(&fixture, None);
    fixture
        .service()
        .finish_side_turn("handoff-test", "done")
        .unwrap();

    let lease = fixture.store.load_lease("handoff-test").unwrap().unwrap();
    assert_eq!(lease.owner_client_id, "ai-parent-42");
}

#[test]
fn side_agent_running_rejects_new_run() {
    let fixture = Fixture::new(HandoffState::SideAgentRunning);
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(fixture.env.clone()), &fixture.invocation(false, None)),
        Err(SideAgentError::AlreadyRunning)
    ));
}

#[test]
fn side_agent_shell_exec_executes_normally() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let turn = run_turn(&fixture, None);
    assert!(!turn.collaborative_handoff);
    let context = ai::application::CollaborativeExecutionContext {
        role: CollaborativeAgentRole::Side,
        policy: CollaborativePolicy::Disabled,
    };
    assert!(!context.should_handoff_shell_exec());
}

#[test]
fn side_agent_waiting_does_not_spawn_new_shell() {
    let fixture = Fixture::new(HandoffState::SideAgentRunning);
    fixture
        .service()
        .request_human_action(
            "handoff-test",
            RequestHumanAction {
                instruction: "Run the integration test".into(),
                reason: "TTY access is needed".into(),
                command_candidates: vec!["cargo test".into()],
                expected_completion: "test passes".into(),
            },
        )
        .unwrap();
    let handoff = fixture.store.load_handoff("handoff-test").unwrap();
    let checkpoint = fixture.store.load_checkpoint("handoff-test").unwrap();
    assert_eq!(handoff.state, HandoffState::SideAgentWaitingForHuman);
    assert_eq!(handoff.shell_generation, 1);
    assert_eq!(checkpoint.command_candidates.len(), 1);
    assert_eq!(checkpoint.command_candidates[0].command, "cargo test");
}

#[test]
fn side_conversation_unique_per_handoff() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    run_turn(&fixture, None);
    let handoff = fixture.store.load_handoff("handoff-test").unwrap();
    let checkpoint = fixture.store.load_checkpoint("handoff-test").unwrap();
    assert_eq!(
        handoff.side_conversation_id,
        checkpoint.side_conversation_id
    );
    assert!(handoff.side_conversation_id.is_some());
}

#[test]
fn stale_handoff_token_is_rejected() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let mut stale = fixture.env.clone();
    stale.generation = 0;
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(stale), &fixture.invocation(false, None)),
        Err(SideAgentError::InvalidToken)
    ));
}

#[test]
fn standalone_child_process_has_no_handoff_token() {
    let mut command = Command::new("sh");
    command.arg("-c").arg("test -z \"$AISH_HANDOFF_TOKEN\"");
    for key in HANDOFF_ENV_KEYS {
        command.env(key, "secret");
    }
    strip_handoff_environment(&mut command);
    assert!(command.status().unwrap().success());
}

#[test]
fn standalone_mode_ignores_handoff_context() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let mut invocation = fixture.invocation(false, None);
    invocation.standalone = true;
    let mut invalid = fixture.env.clone();
    invalid.token = "invalid".into();
    assert_eq!(
        fixture
            .service()
            .dispatch(Some(invalid), &invocation)
            .unwrap(),
        SideAgentDispatch::Standalone
    );
}

#[test]
fn tampered_handoff_id_is_rejected() {
    let fixture = Fixture::new(HandoffState::HumanActive);
    let mut env = fixture.env.clone();
    env.handoff_id = "does-not-exist".into();
    assert!(matches!(
        fixture
            .service()
            .dispatch(Some(env), &fixture.invocation(false, None)),
        Err(SideAgentError::Store(_))
    ));
}
