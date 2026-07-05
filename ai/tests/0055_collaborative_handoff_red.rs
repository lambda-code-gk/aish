// 0055 Collaborative Human Handoff acceptance tests.
// Phase 1 tests are active; later phases remain #[ignore] until implemented.

use ai::adapters::outbound::{FilesystemHandoffStore, SystemHandoffRuntime};
use ai::application::{
    checkpoint_memory_space_id, CollaborativeExecutionContext, CollaborativeShellExecPolicy,
    ParentShellExecRequest,
};
use ai::domain::{
    build_candidate_command, checkpoint_has_required_fields, checkpoint_serialized_field_names,
    close_child_goal_on_control_returned, should_close_child_goal, try_transition,
    validate_shell_token, ChildGoalAchievement, ChildGoalCloseReason, ChildGoalMeta,
    CommandCandidate, CommandCandidateSource, Handoff, HandoffCheckpoint, HandoffEvent,
    HandoffShellSession, HandoffState, RequestedShellExec, CHECKPOINT_REQUIRED_FIELD_NAMES,
    HANDOFF_SCHEMA_VERSION,
};
use ai::domain::{CollaborativeAgentRole, CollaborativePolicy};
use ai::ports::outbound::{
    CheckpointRepository, CommandCandidateStore, HandoffRepository, HandoffRuntime,
    HandoffShellSessionStore, HandoffStoreError, LeaseAcquireRequest, LeaseRepository,
    ShellSessionIssueRequest,
};
use ai::ports::outbound::{
    EnvironmentObservation, EnvironmentObserver, HandoffCandidatePublisher, HumanShellLaunchError,
    HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
    NoopCollaborativeChildGoalService, NoopHandoffCandidatePublisher, NoopParentToolBarrier,
    ParentToolBarrier,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Phase2Trace(Mutex<Vec<String>>);
impl Phase2Trace {
    fn push(&self, value: &str) {
        self.0.lock().unwrap().push(value.into());
    }
    fn values(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct TestBarrier(Arc<Phase2Trace>);
impl ParentToolBarrier for TestBarrier {
    fn wait_for_started_tools(&self) -> Result<(), String> {
        self.0.push("barrier");
        Ok(())
    }
}

struct TestPublisher(Arc<Phase2Trace>);
impl HandoffCandidatePublisher for TestPublisher {
    fn publish(&self, _id: &str, commands: &[String]) -> Result<(), String> {
        assert!(!commands.is_empty());
        self.0.push("candidate");
        Ok(())
    }
}

struct TestObserver;
impl EnvironmentObserver for TestObserver {
    fn observe(&self, cwd: &Path, _start: u64) -> EnvironmentObservation {
        EnvironmentObservation {
            cwd_exists: cwd.is_dir(),
            cwd: cwd.display().to_string(),
            git_head: Some("head".into()),
            git_branch: Some("main".into()),
            git_status: Some(" M file".into()),
            shell_log_end: Some(9),
        }
    }
}

struct TestLauncher {
    root: PathBuf,
    trace: Arc<Phase2Trace>,
}
impl HumanShellLauncher for TestLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        assert!(self
            .root
            .join(&request.handoff_id)
            .join("checkpoint.json")
            .is_file());
        assert_eq!(request.context_version, 1);
        assert!(!request.token.is_empty());
        self.trace.push("shell");
        Ok(HumanShellReturn {
            normal_return: true,
            exit_code: Some(17),
            final_cwd: request.cwd.clone(),
            shell_session_id: "test-session".into(),
            shell_session_dir: request.cwd.clone(),
            shell_log_start: 0,
            shell_log_end: 1,
        })
    }
}

fn phase2_request(cwd: PathBuf) -> ParentShellExecRequest {
    ParentShellExecRequest {
        parent_task_id: "task".into(),
        parent_conversation_id: "conv".into(),
        parent_run_id: "run".into(),
        parent_goal_id: Some("parent-goal".into()),
        parent_goal: "finish phase2".into(),
        parent_request_summary: "run tests".into(),
        conversation_snapshot: "snapshot".into(),
        conversation_summary: "summary".into(),
        work_stage_and_plan: "active work context".into(),
        memory_space_id: None,
        command: "cargo".into(),
        args: vec!["test".into()],
        cwd,
        tool_call_id: "tc".into(),
        shell_log_start: 3,
        suggestion_cache_path: PathBuf::from("/tmp/test-suggestions.json"),
    }
}

fn sample_handoff(id: &str) -> Handoff {
    Handoff {
        id: id.to_string(),
        schema_version: HANDOFF_SCHEMA_VERSION,
        parent_task_id: "task-1".to_string(),
        parent_conversation_id: "conv-1".to_string(),
        parent_run_id: "run-1".to_string(),
        parent_goal_id: Some("goal-parent".to_string()),
        child_goal_id: "goal-child".to_string(),
        side_conversation_id: None,
        state: HandoffState::Creating,
        initial_cwd: "/tmp/work".to_string(),
        final_shell_cwd: None,
        parent_request_summary: "run tests".to_string(),
        requested_shell_execs: vec![RequestedShellExec {
            command: "cargo".to_string(),
            args: vec!["test".to_string()],
            cwd: Some("/tmp/work".to_string()),
            tool_call_id: Some("tc-1".to_string()),
        }],
        pending_human_request: None,
        conversation_snapshot_ref: "snap-1".to_string(),
        conversation_summary: "summary".to_string(),
        checkpoint_ref: "checkpoint.json".to_string(),
        before_observation_ref: "obs-before".to_string(),
        after_observation_ref: None,
        shell_log_start: 10,
        shell_log_end: None,
        shell_generation: 0,
        return_reason: None,
        human_shell_exit_code: None,
        resume_error: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

fn sample_checkpoint(handoff_id: &str) -> HandoffCheckpoint {
    HandoffCheckpoint {
        parent_task_id: "task-1".to_string(),
        parent_conversation_id: "conv-1".to_string(),
        parent_run_id: "run-1".to_string(),
        pending_shell_exec: RequestedShellExec {
            command: "cargo".to_string(),
            args: vec!["test".to_string()],
            cwd: Some("/tmp/work".to_string()),
            tool_call_id: None,
        },
        parent_goal: "finish feature".to_string(),
        child_goal: ChildGoalMeta {
            id: "goal-child".to_string(),
            handoff_id: handoff_id.to_string(),
            parent_goal_id: Some("goal-parent".to_string()),
            work_id: None,
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            achievement: ChildGoalAchievement::Unknown,
        },
        conversation_snapshot: "{}".to_string(),
        conversation_summary: "summary".to_string(),
        cwd: "/tmp/work".to_string(),
        environment_metadata: "{}".to_string(),
        handoff_id: handoff_id.to_string(),
        side_conversation_id: None,
        command_candidates: vec![],
        shell_log_start: 10,
        control_state: HandoffState::Creating,
        provider_metadata: None,
        tool_executions: Vec::new(),
    }
}

#[test]
fn candidate_command_preserves_shell_operators_in_args() {
    let built = build_candidate_command(
        "grep",
        &["foo".to_string(), "||".to_string(), "bar".to_string()],
    );
    assert_eq!(built, "grep 'foo' '||' 'bar'");
}

#[test]
fn checkpoint_contains_required_recovery_fields() {
    let mut checkpoint = sample_checkpoint("ho-1");
    checkpoint.shell_log_start = 0;
    assert!(checkpoint_has_required_fields(&checkpoint));
    let names = checkpoint_serialized_field_names();
    for field in CHECKPOINT_REQUIRED_FIELD_NAMES {
        assert!(names.contains(*field), "missing checkpoint field {field}");
    }
    let json = serde_json::to_string(&checkpoint).expect("serialize checkpoint");
    let roundtrip: HandoffCheckpoint = serde_json::from_str(&json).expect("deserialize checkpoint");
    assert_eq!(roundtrip.shell_log_start, 0);
}

#[test]
fn child_goal_records_control_returned_not_achievement() {
    let mut goal = ChildGoalMeta {
        id: "g1".to_string(),
        handoff_id: "ho-1".to_string(),
        parent_goal_id: None,
        work_id: None,
        auto_root_work_id: None,
        close_reason: None,
        close_state: None,
        achievement: ChildGoalAchievement::Unknown,
    };
    assert!(!should_close_child_goal(HandoffState::Orphaned));
    assert!(should_close_child_goal(HandoffState::Returned));
    close_child_goal_on_control_returned(&mut goal);
    assert_eq!(
        goal.close_reason,
        Some(ChildGoalCloseReason::ControlReturned)
    );
    assert_eq!(goal.achievement, ChildGoalAchievement::Unknown);
}

#[test]
fn command_candidate_source_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let candidate = CommandCandidate {
        id: "cand-1".to_string(),
        command: "cargo test".to_string(),
        description: None,
        source: CommandCandidateSource::ParentAgent,
        source_run_id: Some("run-1".to_string()),
        target_handoff_id: "ho-1".to_string(),
        created_at_ms: 42,
    };
    CommandCandidateStore::append_candidate(&store, "ho-1", &candidate).expect("append");
    let loaded = CommandCandidateStore::list_candidates(&store, "ho-1").expect("list");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].source, CommandCandidateSource::ParentAgent);
    assert_eq!(loaded[0].command, "cargo test");

    let mismatched = CommandCandidate {
        target_handoff_id: "ho-other".to_string(),
        ..candidate
    };
    let err = CommandCandidateStore::append_candidate(&store, "ho-1", &mismatched)
        .expect_err("mismatched target");
    assert!(matches!(err, HandoffStoreError::InvalidHandoffId));
}

#[test]
fn handoff_lease_rejects_concurrent_owner() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let handoff = sample_handoff("ho-lease");
    HandoffRepository::save_handoff(&store, &handoff).expect("save handoff");

    let base = LeaseAcquireRequest {
        owner_client_id: "client-a".to_string(),
        owner_process_id: 100,
        owner_tty: None,
        owner_host: "localhost".to_string(),
        owner_uid: 1000,
        now_ms: 1_000,
        lease_timeout_ms: 60_000,
    };
    LeaseRepository::try_acquire_lease(&store, "ho-lease", &base).expect("first lease");

    let other = LeaseAcquireRequest {
        owner_client_id: "client-b".to_string(),
        owner_process_id: base.owner_process_id,
        owner_tty: base.owner_tty.clone(),
        owner_host: base.owner_host.clone(),
        owner_uid: base.owner_uid,
        now_ms: base.now_ms,
        lease_timeout_ms: base.lease_timeout_ms,
    };
    let err = LeaseRepository::try_acquire_lease(&store, "ho-lease", &other)
        .expect_err("second lease must fail");
    assert!(matches!(err, HandoffStoreError::LeaseConflict));

    let same_client_other_process = LeaseAcquireRequest {
        owner_process_id: 200,
        ..base
    };
    let err = LeaseRepository::try_acquire_lease(&store, "ho-lease", &same_client_other_process)
        .expect_err("different process must fail");
    assert!(matches!(err, HandoffStoreError::LeaseConflict));
}

#[test]
fn handoff_store_rejects_unsafe_handoff_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let mut handoff = sample_handoff("../escape");
    let err = HandoffRepository::save_handoff(&store, &handoff).expect_err("unsafe id");
    assert!(matches!(err, HandoffStoreError::InvalidHandoffId));
    handoff.id = "ho-safe".to_string();
    HandoffRepository::save_handoff(&store, &handoff).expect("safe id");
}

#[test]
fn handoff_state_transitions_are_validated() {
    assert_eq!(
        try_transition(HandoffState::Creating, HandoffEvent::ShellReady).unwrap(),
        HandoffState::HumanActive
    );
    assert_eq!(
        try_transition(HandoffState::HumanActive, HandoffEvent::StartSideAgent).unwrap(),
        HandoffState::SideAgentRunning
    );
    assert_eq!(
        try_transition(
            HandoffState::SideAgentWaitingForHuman,
            HandoffEvent::SideAgentResumed
        )
        .unwrap(),
        HandoffState::SideAgentRunning
    );
    assert_eq!(
        try_transition(
            HandoffState::SideAgentRunning,
            HandoffEvent::SideAgentReturned
        )
        .unwrap(),
        HandoffState::HumanActive
    );
    assert_eq!(
        try_transition(HandoffState::Orphaned, HandoffEvent::Resume).unwrap(),
        HandoffState::HumanActive
    );
    assert!(try_transition(HandoffState::Orphaned, HandoffEvent::ShellReady).is_err());
    assert_eq!(
        try_transition(HandoffState::Creating, HandoffEvent::ShellLaunchFailed).unwrap(),
        HandoffState::Cancelled
    );
}

#[test]
fn handoff_store_persists_token_hash_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let token = "super-secret-handoff-token";
    HandoffShellSessionStore::append_shell_session(
        &store,
        "ho-token",
        &ShellSessionIssueRequest {
            generation: 1,
            token_plaintext: token.to_string(),
            now_ms: 1,
        },
    )
    .expect("append session");

    let raw = std::fs::read_to_string(dir.path().join("ho-token/shell_sessions.jsonl"))
        .expect("read sessions");
    assert!(!raw.contains(token));
    assert!(raw.contains("token_hash"));
}

#[test]
fn shell_session_generation_invalidates_old_token() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let old_token = "token-gen-1";
    let new_token = "token-gen-2";
    HandoffShellSessionStore::append_shell_session(
        &store,
        "ho-gen",
        &ShellSessionIssueRequest {
            generation: 1,
            token_plaintext: old_token.to_string(),
            now_ms: 1,
        },
    )
    .expect("gen1");
    HandoffShellSessionStore::append_shell_session(
        &store,
        "ho-gen",
        &ShellSessionIssueRequest {
            generation: 2,
            token_plaintext: new_token.to_string(),
            now_ms: 2,
        },
    )
    .expect("gen2");
    let sessions = HandoffShellSessionStore::list_shell_sessions(&store, "ho-gen").expect("list");
    assert!(!validate_shell_token(&sessions, old_token, 1));
    assert!(validate_shell_token(&sessions, new_token, 2));

    let err = HandoffShellSessionStore::append_shell_session(
        &store,
        "ho-gen",
        &ShellSessionIssueRequest {
            generation: 1,
            token_plaintext: "reused-gen".to_string(),
            now_ms: 3,
        },
    )
    .expect_err("duplicate generation");
    assert!(matches!(err, HandoffStoreError::InvalidShellGeneration));
}

#[test]
fn shell_session_generation_rejects_overflow_at_max() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let max_session = HandoffShellSession {
        generation: u32::MAX,
        token_hash: "deadbeef".to_string(),
        created_at_ms: 1,
    };
    let sessions_path = dir.path().join("ho-max/shell_sessions.jsonl");
    std::fs::create_dir_all(sessions_path.parent().expect("parent")).expect("mkdir");
    let line = serde_json::to_string(&max_session).expect("json");
    std::fs::write(&sessions_path, format!("{line}\n")).expect("write");
    let err = HandoffShellSessionStore::append_shell_session(
        &store,
        "ho-max",
        &ShellSessionIssueRequest {
            generation: 2,
            token_plaintext: "overflow".to_string(),
            now_ms: 2,
        },
    )
    .expect_err("overflow generation");
    assert!(matches!(err, HandoffStoreError::InvalidShellGeneration));
}

#[test]
fn checkpoint_rejects_mismatched_handoff_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let mut checkpoint = sample_checkpoint("ho-a");
    checkpoint.handoff_id = "ho-b".to_string();
    let err = CheckpointRepository::save_checkpoint(&store, "ho-a", &checkpoint)
        .expect_err("mismatched checkpoint id");
    assert!(matches!(err, HandoffStoreError::InvalidHandoffId));
}

#[test]
fn checkpoint_persisted_before_human_shell_spawn() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let trace = Arc::new(Phase2Trace::default());
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let launcher = TestLauncher {
        root: dir.path().into(),
        trace: trace.clone(),
    };
    let barrier = TestBarrier(trace.clone());
    let publisher = TestPublisher(trace.clone());
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &barrier,
        &publisher,
        &NoopCollaborativeChildGoalService,
        &SystemHandoffRuntime,
    );
    policy
        .intercept(phase2_request(work.path().into()))
        .unwrap();
    assert_eq!(trace.values(), vec!["barrier", "candidate", "shell"]);
}

#[test]
fn handoff_checkpoint_stores_parent_memory_space_id() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(dir.path().to_path_buf());
    let launcher = TestLauncher {
        root: dir.path().into(),
        trace: Arc::new(Phase2Trace::default()),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &NoopCollaborativeChildGoalService,
        &SystemHandoffRuntime,
    );
    let mut request = phase2_request(work.path().to_path_buf());
    request.memory_space_id = Some("project_parent".into());
    let result = policy.intercept(request).unwrap();
    let checkpoint = store.load_checkpoint(&result.handoff_id).unwrap();
    assert_eq!(
        checkpoint_memory_space_id(&checkpoint).as_deref(),
        Some("project_parent")
    );
}

#[test]
fn collaborative_flag_enables_parent_policy() {
    use clap::Parser;
    let cli =
        ai::clap_cli::AiCli::try_parse_from(["ai", "--collaborative", "ask", "task"]).unwrap();
    assert!(cli.collaborative);
    assert!(CollaborativeExecutionContext::parent_enabled().should_handoff_shell_exec());
}

#[test]
fn handoff_completes_normal_parent_resume_flow() {
    use ai::application::{RecoveryOwner, ResumeReturnedParent};

    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let trace = Arc::new(Phase2Trace::default());
    let store = FilesystemHandoffStore::new(dir.path().into());
    let launcher = TestLauncher {
        root: dir.path().into(),
        trace: trace.clone(),
    };
    let barrier = TestBarrier(trace.clone());
    let publisher = TestPublisher(trace);
    let runtime = SystemHandoffRuntime;
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &barrier,
        &publisher,
        &NoopCollaborativeChildGoalService,
        &runtime,
    );
    let result = policy
        .intercept(phase2_request(work.path().into()))
        .unwrap();
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::Returned
    );
    assert!(
        store.load_lease(&result.handoff_id).unwrap().is_none(),
        "lease must be released after human shell returns"
    );
    let resume = ResumeReturnedParent::new(&store, &TestObserver, &runtime);
    resume
        .prepare(
            &result.handoff_id,
            &RecoveryOwner {
                client_id: format!("ai-parent-{}", runtime.process_id()),
                process_id: runtime.process_id(),
                tty: None,
            },
        )
        .unwrap();
    resume.finish(&result.handoff_id, Ok(())).unwrap();
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::Completed
    );
}

#[test]
fn handoff_records_final_shell_cwd_on_return() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let trace = Arc::new(Phase2Trace::default());
    let store = FilesystemHandoffStore::new(dir.path().into());
    let launcher = TestLauncher {
        root: dir.path().into(),
        trace: trace.clone(),
    };
    let barrier = TestBarrier(trace.clone());
    let publisher = TestPublisher(trace);
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &barrier,
        &publisher,
        &NoopCollaborativeChildGoalService,
        &SystemHandoffRuntime,
    );
    let result = policy
        .intercept(phase2_request(work.path().into()))
        .unwrap();
    assert_eq!(
        store
            .load_handoff(&result.handoff_id)
            .unwrap()
            .final_shell_cwd
            .as_deref(),
        work.path().to_str()
    );
}

#[test]
fn missing_cwd_rejects_human_shell_spawn() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let trace = Arc::new(Phase2Trace::default());
    let store = FilesystemHandoffStore::new(dir.path().join("store"));
    let launcher = TestLauncher {
        root: dir.path().join("store"),
        trace: trace.clone(),
    };
    let barrier = TestBarrier(trace.clone());
    let publisher = TestPublisher(trace.clone());
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &barrier,
        &publisher,
        &NoopCollaborativeChildGoalService,
        &SystemHandoffRuntime,
    );
    assert!(policy.intercept(phase2_request(missing)).is_err());
    assert!(!trace.values().contains(&"shell".into()));
}

#[test]
fn non_parent_role_skips_handoff() {
    let ctx = CollaborativeExecutionContext {
        role: CollaborativeAgentRole::Side,
        policy: CollaborativePolicy::Enabled,
    };
    assert!(!ctx.should_handoff_shell_exec());
}

#[test]
fn collaborative_audit_events_are_emitted() {
    use ai::adapters::outbound::FilesystemHandoffStore;
    use ai::application::{
        CollaborativeExecutionContext, CollaborativeShellExecPolicy, ParentShellExecRequest,
    };
    use ai::domain::{CollaborativeAuditKind, HandoffState};
    use ai::ports::outbound::{
        EnvironmentObservation, EnvironmentObserver, HandoffRepository, HumanShellLaunchError,
        HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
        NoopCollaborativeChildGoalService, NoopHandoffCandidatePublisher, NoopParentToolBarrier,
    };
    use std::path::{Path, PathBuf};

    struct Observer;
    impl EnvironmentObserver for Observer {
        fn observe(&self, cwd: &Path, _: u64) -> EnvironmentObservation {
            EnvironmentObservation {
                cwd_exists: cwd.is_dir(),
                cwd: cwd.display().to_string(),
                git_head: None,
                git_branch: None,
                git_status: None,
                shell_log_end: Some(1),
            }
        }
    }
    struct Launcher;
    impl HumanShellLauncher for Launcher {
        fn launch_and_wait(
            &self,
            _: &HumanShellLaunchRequest,
        ) -> Result<HumanShellReturn, HumanShellLaunchError> {
            Ok(HumanShellReturn {
                normal_return: true,
                exit_code: Some(0),
                final_cwd: PathBuf::from("/tmp/work"),
                shell_session_id: "test-session".into(),
                shell_session_dir: PathBuf::from("/tmp/work"),
                shell_log_start: 0,
                shell_log_end: 1,
            })
        }
    }
    struct Runtime;
    impl ai::ports::outbound::HandoffRuntime for Runtime {
        fn now_ms(&self) -> u64 {
            1
        }
        fn unique_id(&self, prefix: &str) -> String {
            format!("{prefix}-audit")
        }
        fn secure_token(&self) -> Result<String, String> {
            Ok("token".into())
        }
    }

    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("workspace");
    std::fs::create_dir(&cwd).unwrap();
    let store = FilesystemHandoffStore::new(root.path().join("handoffs"));
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &Launcher,
        &Observer,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &NoopCollaborativeChildGoalService,
        &Runtime,
    );
    let request = ParentShellExecRequest {
        parent_task_id: "task".into(),
        parent_conversation_id: "conv".into(),
        parent_run_id: "run".into(),
        parent_goal_id: None,
        parent_goal: "goal".into(),
        parent_request_summary: "summary".into(),
        conversation_snapshot: "{}".into(),
        conversation_summary: "summary".into(),
        work_stage_and_plan: String::new(),
        memory_space_id: None,
        command: "echo".into(),
        args: vec!["hi".into()],
        cwd: cwd.clone(),
        tool_call_id: "tc".into(),
        shell_log_start: 0,
        suggestion_cache_path: root.path().join("suggestions.json"),
    };
    policy.intercept(request).unwrap();
    let handoff = store.list_handoffs().unwrap().pop().unwrap();
    assert_eq!(handoff.state, HandoffState::Returned);
    let events_path = root
        .path()
        .join("handoffs")
        .join(&handoff.id)
        .join("events.jsonl");
    let events = std::fs::read_to_string(events_path).unwrap();
    for kind in [
        CollaborativeAuditKind::HandoffCreated,
        CollaborativeAuditKind::CandidateRegistered,
        CollaborativeAuditKind::LeaseAcquired,
        CollaborativeAuditKind::HumanShellStarted,
        CollaborativeAuditKind::HumanShellReturned,
        CollaborativeAuditKind::LeaseLost,
    ] {
        let needle = serde_json::to_string(&kind)
            .unwrap()
            .trim_matches('"')
            .to_string();
        assert!(
            events.contains(&needle),
            "missing audit event {needle}: {events}"
        );
    }
}

#[test]
fn collaborative_config_defaults_match_spec() {
    use ai::adapters::outbound::toml_config::CollaborativeConfig;
    let cfg = CollaborativeConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.heartbeat_interval_secs, 30);
    assert_eq!(cfg.lease_timeout_secs, 120);
    assert_eq!(cfg.recent_parent_turns, 6);
    assert_eq!(cfg.recent_side_turns, 8);
    assert_eq!(cfg.summary_token_limit, 4096);
    assert!(cfg.prompt_template.contains("{state}"));
}

#[test]
fn handoff_token_not_in_replay_output() {
    use aish_replay::{replay_show, LogEvent};
    let token = "opaque-handoff-token-0123456789abcdef";
    let events = vec![
        LogEvent::shell_command_start(1, "2026-01-01T00:00:00Z", &format!("echo {token}")),
        LogEvent::stdout_indexed("ok\n", 1),
        LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
    ];
    let out = replay_show(&events, 1, false).unwrap();
    assert!(!out.contains(token));
    assert_eq!(out, "ok\n");
}

#[test]
fn handoff_token_redacted_from_shell_log() {
    let token = "opaque-handoff-token-0123456789abcdef";
    let redacted = aish_replay::sanitize_log_text_with_secrets(
        &format!("echo {token}\nAISH_HANDOFF_TOKEN={token}"),
        &[token],
    );
    assert!(!redacted.contains(token));
    assert!(redacted.contains("[REDACTED]"));
}

struct LeaseTransferLauncher {
    root: PathBuf,
    human_shell_pid: u32,
}

impl HumanShellLauncher for LeaseTransferLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        let lease_path = self.root.join(&request.handoff_id).join("lease.json");
        let mut lease: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&lease_path).unwrap()).unwrap();
        let object = lease.as_object_mut().unwrap();
        object.insert(
            "owner_client_id".into(),
            format!("aish-human-shell-{}", self.human_shell_pid).into(),
        );
        object.insert("owner_process_id".into(), self.human_shell_pid.into());
        object.insert(
            "lease_expires_at_ms".into(),
            (1_000_000u64 + 120_000).into(),
        );
        std::fs::write(&lease_path, serde_json::to_vec_pretty(&lease).unwrap()).unwrap();
        Ok(HumanShellReturn {
            normal_return: true,
            exit_code: Some(0),
            final_cwd: request.cwd.clone(),
            shell_session_id: "test-session".into(),
            shell_session_dir: request.cwd.clone(),
            shell_log_start: 0,
            shell_log_end: 1,
        })
    }
}

#[derive(Debug)]
struct ResumeRuntime {
    now: u64,
    process_id: u32,
}

impl ai::ports::outbound::HandoffRuntime for ResumeRuntime {
    fn now_ms(&self) -> u64 {
        self.now
    }

    fn unique_id(&self, prefix: &str) -> String {
        format!("{prefix}-resume")
    }

    fn secure_token(&self) -> Result<String, String> {
        Ok("resume-token".into())
    }

    fn host_id(&self) -> String {
        SystemHandoffRuntime.host_id()
    }

    fn effective_uid(&self) -> u32 {
        SystemHandoffRuntime.effective_uid()
    }

    fn process_id(&self) -> u32 {
        self.process_id
    }
}

#[test]
fn human_shell_return_releases_lease_for_parent_resume() {
    use ai::application::{RecoveryOwner, ResumeReturnedParent};

    let root = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(root.path().join("handoffs"));
    let human_shell_pid = 424_242u32;
    let launcher = LeaseTransferLauncher {
        root: root.path().join("handoffs"),
        human_shell_pid,
    };
    let runtime = ResumeRuntime {
        now: 1_000,
        process_id: 9001,
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &NoopCollaborativeChildGoalService,
        &runtime,
    );
    let result = policy
        .intercept(phase2_request(cwd.path().to_path_buf()))
        .unwrap();
    assert_eq!(
        result.execution_outcome,
        aibe_protocol::HandoffExecutionOutcome::HumanControlReturned
    );
    assert!(
        store.load_lease(&result.handoff_id).unwrap().is_none(),
        "human shell lease must be released on normal return"
    );

    let resume = ResumeReturnedParent::new(&store, &TestObserver, &runtime);
    resume
        .prepare(
            &result.handoff_id,
            &RecoveryOwner {
                client_id: format!("ai-parent-{}", runtime.process_id),
                process_id: runtime.process_id,
                tty: None,
            },
        )
        .expect("parent must re-acquire lease after human shell return");
    assert!(store.load_lease(&result.handoff_id).unwrap().is_some());
    resume.finish(&result.handoff_id, Ok(())).unwrap();
    assert!(
        store.load_lease(&result.handoff_id).unwrap().is_none(),
        "parent resume must release lease when completed"
    );
    assert_eq!(
        store.load_handoff(&result.handoff_id).unwrap().state,
        HandoffState::Completed
    );
}

struct FailingLauncher {
    root: PathBuf,
}

impl HumanShellLauncher for FailingLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        assert!(self
            .root
            .join(&request.handoff_id)
            .join("checkpoint.json")
            .is_file());
        Err(HumanShellLaunchError::Failed("spawn failed".into()))
    }
}

struct OrphanedLauncher {
    root: PathBuf,
}

impl HumanShellLauncher for OrphanedLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        assert!(self
            .root
            .join(&request.handoff_id)
            .join("checkpoint.json")
            .is_file());
        Err(HumanShellLaunchError::MissingReturnMarker)
    }
}

struct TrackingChildGoalService {
    inner: ai::adapters::outbound::AibeCollaborativeChildGoalService<MockWorkClientForHandoff>,
    close_calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

struct MockWorkClientForHandoff {
    active: std::sync::Arc<std::sync::Mutex<Option<u64>>>,
}

impl ai::ports::outbound::WorkClient for MockWorkClientForHandoff {
    fn work_query(
        &self,
        _session_id: &str,
        _context: &aibe_protocol::MemoryContext,
    ) -> Result<aibe_protocol::ClientResponse, ai::ports::outbound::AgentError> {
        Ok(aibe_protocol::ClientResponse::WorkQueryResult(
            aibe_protocol::WorkQueryResponseBody {
                id: "q".into(),
                snapshot: aibe_protocol::WorkSnapshotDto {
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
                *self.active.lock().unwrap() = Some(1);
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Start,
                    work_id: Some(1),
                    previous_work_id: None,
                }
            }
            aibe_protocol::WorkOperationDto::Push { .. } => {
                *self.active.lock().unwrap() = Some(2);
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Push,
                    work_id: Some(2),
                    previous_work_id: Some(1),
                }
            }
            aibe_protocol::WorkOperationDto::Pop => {
                let current = *self.active.lock().unwrap();
                *self.active.lock().unwrap() = if current == Some(2) { Some(1) } else { None };
                WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Pop,
                    work_id: current,
                    previous_work_id: None,
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

impl ai::ports::outbound::CollaborativeChildGoalService for TrackingChildGoalService {
    fn create_child_goal(
        &self,
        meta: &mut ai::domain::ChildGoalMeta,
        cwd: &std::path::Path,
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
        meta: &ai::domain::ChildGoalMeta,
        cwd: &std::path::Path,
        reason: ai::domain::ChildGoalCloseReason,
    ) -> Result<(), ai::ports::outbound::CollaborativeChildGoalError> {
        self.close_calls
            .lock()
            .unwrap()
            .push(format!("{reason:?}:{:?}", meta.work_id));
        self.inner.close_child_goal(meta, cwd, reason)
    }
}

#[test]
fn handoff_creates_child_work_without_active_work_and_pops_on_return() {
    use ai::domain::ChildGoalCloseState;

    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(dir.path().into());
    let active = std::sync::Arc::new(std::sync::Mutex::new(None));
    let close_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let child_goal = TrackingChildGoalService {
        inner: ai::adapters::outbound::AibeCollaborativeChildGoalService::new(
            MockWorkClientForHandoff {
                active: active.clone(),
            },
            "session".into(),
            "space".into(),
        ),
        close_calls: close_calls.clone(),
    };
    let launcher = TestLauncher {
        root: dir.path().into(),
        trace: Arc::new(Phase2Trace::default()),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &child_goal,
        &SystemHandoffRuntime,
    );
    let result = policy
        .intercept(phase2_request(work.path().into()))
        .unwrap();
    let checkpoint = store.load_checkpoint(&result.handoff_id).unwrap();
    assert_eq!(checkpoint.child_goal.auto_root_work_id, Some(1));
    assert_eq!(checkpoint.child_goal.work_id, Some(2));
    assert_eq!(
        checkpoint.child_goal.close_state,
        Some(ChildGoalCloseState::Completed)
    );
    assert_eq!(close_calls.lock().unwrap().len(), 1);
    assert!(
        close_calls.lock().unwrap()[0].starts_with("ControlReturned"),
        "unexpected close call: {:?}",
        close_calls.lock().unwrap()
    );
}

#[test]
fn handoff_shell_launch_failure_compensates_child_work() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(dir.path().join("store"));
    let active = std::sync::Arc::new(std::sync::Mutex::new(None));
    let close_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let child_goal = TrackingChildGoalService {
        inner: ai::adapters::outbound::AibeCollaborativeChildGoalService::new(
            MockWorkClientForHandoff {
                active: active.clone(),
            },
            "session".into(),
            "space".into(),
        ),
        close_calls: close_calls.clone(),
    };
    let launcher = FailingLauncher {
        root: dir.path().join("store"),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &child_goal,
        &SystemHandoffRuntime,
    );
    let error = policy
        .intercept(phase2_request(work.path().to_path_buf()))
        .expect_err("launch failure");
    assert!(matches!(
        error,
        ai::application::CollaborativeHandoffError::Launch(_)
    ));
    assert_eq!(close_calls.lock().unwrap().len(), 1);
    assert!(
        close_calls.lock().unwrap()[0].starts_with("Compensated"),
        "expected compensated close, got {:?}",
        close_calls.lock().unwrap()
    );
    assert_eq!(*active.lock().unwrap(), Some(1));
}

#[test]
fn handoff_orphaned_shell_exit_preserves_child_work() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(dir.path().into());
    let active = std::sync::Arc::new(std::sync::Mutex::new(None));
    let close_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let child_goal = TrackingChildGoalService {
        inner: ai::adapters::outbound::AibeCollaborativeChildGoalService::new(
            MockWorkClientForHandoff {
                active: active.clone(),
            },
            "session".into(),
            "space".into(),
        ),
        close_calls: close_calls.clone(),
    };
    let launcher = OrphanedLauncher {
        root: dir.path().into(),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &child_goal,
        &SystemHandoffRuntime,
    );
    let error = policy
        .intercept(phase2_request(work.path().to_path_buf()))
        .expect_err("orphaned shell exit");
    assert!(matches!(
        error,
        ai::application::CollaborativeHandoffError::Launch(
            HumanShellLaunchError::MissingReturnMarker
        )
    ));
    let handoff = store.list_handoffs().unwrap().into_iter().next().unwrap();
    assert_eq!(handoff.state, HandoffState::Orphaned);
    let checkpoint = store.load_checkpoint(&handoff.id).unwrap();
    assert_eq!(checkpoint.child_goal.work_id, Some(2));
    assert!(checkpoint.child_goal.close_state.is_none());
    assert!(close_calls.lock().unwrap().is_empty());
}

struct MockWorkClientPushFails {
    active: std::sync::Arc<std::sync::Mutex<Option<u64>>>,
}

impl ai::ports::outbound::WorkClient for MockWorkClientPushFails {
    fn work_query(
        &self,
        _session_id: &str,
        _context: &aibe_protocol::MemoryContext,
    ) -> Result<aibe_protocol::ClientResponse, ai::ports::outbound::AgentError> {
        Ok(aibe_protocol::ClientResponse::WorkQueryResult(
            aibe_protocol::WorkQueryResponseBody {
                id: "q".into(),
                snapshot: aibe_protocol::WorkSnapshotDto {
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
        match operation {
            aibe_protocol::WorkOperationDto::Start { .. } => {
                *self.active.lock().unwrap() = Some(1);
                Ok(aibe_protocol::ClientResponse::WorkApplyResult(
                    WorkApplyResponseBody {
                        id: "a".into(),
                        snapshot: WorkSnapshotDto {
                            revision: 1,
                            active_work_id: Some(1),
                            stack: vec![],
                            works: vec![],
                            entries: vec![],
                        },
                        outcome: WorkMutationOutcomeDto {
                            kind: WorkMutationKindDto::Start,
                            work_id: Some(1),
                            previous_work_id: None,
                        },
                    },
                ))
            }
            aibe_protocol::WorkOperationDto::Push { .. } => Err(
                ai::ports::outbound::AgentError::Request("push rejected".into()),
            ),
            aibe_protocol::WorkOperationDto::Pop => {
                let current = *self.active.lock().unwrap();
                *self.active.lock().unwrap() = None;
                Ok(aibe_protocol::ClientResponse::WorkApplyResult(
                    WorkApplyResponseBody {
                        id: "a".into(),
                        snapshot: WorkSnapshotDto {
                            revision: 1,
                            active_work_id: None,
                            stack: vec![],
                            works: vec![],
                            entries: vec![],
                        },
                        outcome: WorkMutationOutcomeDto {
                            kind: WorkMutationKindDto::Pop,
                            work_id: current,
                            previous_work_id: None,
                        },
                    },
                ))
            }
            _ => panic!("unexpected work op"),
        }
    }
}

#[test]
fn handoff_create_failure_compensates_auto_root_work() {
    use ai::domain::ChildGoalCloseState;

    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(dir.path().join("store"));
    let active = std::sync::Arc::new(std::sync::Mutex::new(None));
    let child_goal = ai::adapters::outbound::AibeCollaborativeChildGoalService::new(
        MockWorkClientPushFails {
            active: active.clone(),
        },
        "session".into(),
        "space".into(),
    );
    let launcher = TestLauncher {
        root: dir.path().join("store"),
        trace: Arc::new(Phase2Trace(Mutex::new(Vec::new()))),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &child_goal,
        &SystemHandoffRuntime,
    );
    let result = policy
        .intercept(phase2_request(work.path().to_path_buf()))
        .expect("handoff continues without child work");
    let checkpoint = store.load_checkpoint(&result.handoff_id).unwrap();
    assert!(checkpoint.child_goal.work_id.is_none());
    assert_eq!(checkpoint.child_goal.auto_root_work_id, Some(1));
    assert_eq!(
        checkpoint.child_goal.close_state,
        Some(ChildGoalCloseState::Completed)
    );
    assert!(active.lock().unwrap().is_none());
}

struct MockWorkClientPushFailsFirstPop {
    active: std::sync::Arc<std::sync::Mutex<Option<u64>>>,
    pop_attempts: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl ai::ports::outbound::WorkClient for MockWorkClientPushFailsFirstPop {
    fn work_query(
        &self,
        _session_id: &str,
        _context: &aibe_protocol::MemoryContext,
    ) -> Result<aibe_protocol::ClientResponse, ai::ports::outbound::AgentError> {
        Ok(aibe_protocol::ClientResponse::WorkQueryResult(
            aibe_protocol::WorkQueryResponseBody {
                id: "q".into(),
                snapshot: aibe_protocol::WorkSnapshotDto {
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
        use std::sync::atomic::Ordering;
        match operation {
            aibe_protocol::WorkOperationDto::Start { .. } => {
                *self.active.lock().unwrap() = Some(1);
                Ok(aibe_protocol::ClientResponse::WorkApplyResult(
                    WorkApplyResponseBody {
                        id: "a".into(),
                        snapshot: WorkSnapshotDto {
                            revision: 1,
                            active_work_id: Some(1),
                            stack: vec![],
                            works: vec![],
                            entries: vec![],
                        },
                        outcome: WorkMutationOutcomeDto {
                            kind: WorkMutationKindDto::Start,
                            work_id: Some(1),
                            previous_work_id: None,
                        },
                    },
                ))
            }
            aibe_protocol::WorkOperationDto::Push { .. } => Err(
                ai::ports::outbound::AgentError::Request("push rejected".into()),
            ),
            aibe_protocol::WorkOperationDto::Pop => {
                let attempt = self.pop_attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt == 1 {
                    return Err(ai::ports::outbound::AgentError::Request(
                        "active work mismatch: active=99 expected=1".into(),
                    ));
                }
                let current = *self.active.lock().unwrap();
                *self.active.lock().unwrap() = None;
                Ok(aibe_protocol::ClientResponse::WorkApplyResult(
                    WorkApplyResponseBody {
                        id: "a".into(),
                        snapshot: WorkSnapshotDto {
                            revision: 1,
                            active_work_id: None,
                            stack: vec![],
                            works: vec![],
                            entries: vec![],
                        },
                        outcome: WorkMutationOutcomeDto {
                            kind: WorkMutationKindDto::Pop,
                            work_id: current,
                            previous_work_id: None,
                        },
                    },
                ))
            }
            _ => panic!("unexpected work op"),
        }
    }
}

#[test]
fn handoff_create_failure_preserves_resume_error_after_shell_ready() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(dir.path().join("store"));
    let active = std::sync::Arc::new(std::sync::Mutex::new(None));
    let pop_attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let child_goal = ai::adapters::outbound::AibeCollaborativeChildGoalService::new(
        MockWorkClientPushFailsFirstPop {
            active: active.clone(),
            pop_attempts: pop_attempts.clone(),
        },
        "session".into(),
        "space".into(),
    );
    let launcher = TestLauncher {
        root: dir.path().join("store"),
        trace: Arc::new(Phase2Trace(Mutex::new(Vec::new()))),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &store,
        &launcher,
        &TestObserver,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &child_goal,
        &SystemHandoffRuntime,
    );
    let result = policy
        .intercept(phase2_request(work.path().to_path_buf()))
        .expect("handoff continues after create/compensate errors");
    let handoff = store.load_handoff(&result.handoff_id).unwrap();
    assert!(
        handoff
            .resume_error
            .as_deref()
            .is_some_and(|message| message.contains("child_goal_create:")),
        "resume_error must survive ShellReady save, got {:?}",
        handoff.resume_error
    );
    assert_eq!(pop_attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
}
