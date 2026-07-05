use std::path::PathBuf;
use std::sync::Mutex;

use ai::adapters::outbound::FilesystemHandoffStore;
use ai::application::{
    finalize_parent_resume_tool_tracking, has_unknown_tools, list_recoverable_handoffs,
    record_handoff_tool_running, select_recoverable_handoff, CollaborativeExecutionContext,
    CollaborativeRecoveryError, CollaborativeShellExecPolicy, MarkOrphaned, ParentShellExecRequest,
    ReconcileStaleHandoffs, RecoveryOwner, ResumeOrphanedHandoff, ResumeReturnedParent,
    ResumedHandoffSync, ReturnControlFromShell,
};
use ai::domain::{
    validate_shell_token, ChildGoalAchievement, ChildGoalMeta, Handoff, HandoffCheckpoint,
    HandoffState, RecoverableToolExecution, RecoverableToolStatus, RequestedShellExec,
    HANDOFF_SCHEMA_VERSION,
};
use ai::ports::outbound::{
    CheckpointRepository, EnvironmentObservation, EnvironmentObserver, HandoffRepository,
    HandoffRuntime, HandoffShellSessionStore, HumanShellLaunchError, HumanShellLaunchRequest,
    HumanShellLauncher, HumanShellReturn, LeaseAcquireRequest, LeaseRepository,
    NoopCollaborativeChildGoalService, NoopHandoffCandidatePublisher, NoopParentToolBarrier,
    ShellSessionIssueRequest,
};

#[derive(Debug)]
struct Runtime {
    now: u64,
    owner_alive: bool,
}

impl HandoffRuntime for Runtime {
    fn now_ms(&self) -> u64 {
        self.now
    }

    fn unique_id(&self, prefix: &str) -> String {
        format!("{prefix}-test")
    }

    fn secure_token(&self) -> Result<String, String> {
        Ok("rotated-token".into())
    }

    fn host_id(&self) -> String {
        "test-host".into()
    }

    fn effective_uid(&self) -> u32 {
        1000
    }

    fn process_is_alive(&self, _process_id: u32) -> bool {
        self.owner_alive
    }
}

#[derive(Debug)]
struct Launcher {
    requests: Mutex<Vec<HumanShellLaunchRequest>>,
    result: Result<HumanShellReturn, HumanShellLaunchError>,
}

impl Launcher {
    fn returning(cwd: &str) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            result: Ok(HumanShellReturn {
                normal_return: true,
                exit_code: Some(0),
                final_cwd: cwd.into(),
                shell_session_id: "test-session".into(),
                shell_session_dir: cwd.into(),
                shell_log_start: 0,
                shell_log_end: 1,
            }),
        }
    }
}

impl HumanShellLauncher for Launcher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        self.requests.lock().unwrap().push(request.clone());
        match &self.result {
            Ok(result) => Ok(result.clone()),
            Err(HumanShellLaunchError::MissingReturnMarker) => {
                Err(HumanShellLaunchError::MissingReturnMarker)
            }
            Err(HumanShellLaunchError::MissingCwd(path)) => {
                Err(HumanShellLaunchError::MissingCwd(path.clone()))
            }
            Err(HumanShellLaunchError::Failed(message)) => {
                Err(HumanShellLaunchError::Failed(message.clone()))
            }
        }
    }
}

struct Observer;

impl EnvironmentObserver for Observer {
    fn observe(&self, cwd: &std::path::Path, _shell_log_start: u64) -> EnvironmentObservation {
        EnvironmentObservation {
            cwd_exists: cwd.is_dir(),
            cwd: cwd.display().to_string(),
            git_head: None,
            git_branch: None,
            git_status: None,
            shell_log_end: Some(20),
        }
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    store: FilesystemHandoffStore,
    cwd: String,
}

impl Fixture {
    fn new(id: &str, state: HandoffState) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let store = FilesystemHandoffStore::new(temp.path().join("handoffs"));
        let cwd = cwd.display().to_string();
        save_handoff(&store, id, state, &cwd);
        Self {
            _temp: temp,
            store,
            cwd,
        }
    }

    fn lease(&self, id: &str, now_ms: u64, timeout_ms: u64) {
        self.store
            .try_acquire_lease(
                id,
                &LeaseAcquireRequest {
                    owner_client_id: "old-owner".into(),
                    owner_process_id: 4242,
                    owner_tty: None,
                    owner_host: "test-host".into(),
                    owner_uid: 1000,
                    now_ms,
                    lease_timeout_ms: timeout_ms,
                },
            )
            .unwrap();
    }
}

fn owner() -> RecoveryOwner {
    RecoveryOwner {
        client_id: "resume-owner".into(),
        process_id: 9000,
        tty: None,
    }
}

fn parent_request(cwd: &str) -> ParentShellExecRequest {
    ParentShellExecRequest {
        parent_task_id: "parent-task".into(),
        parent_conversation_id: "parent-conversation".into(),
        parent_run_id: "parent-run".into(),
        parent_goal_id: Some("parent-goal".into()),
        parent_goal: "finish Phase 4 safely".into(),
        parent_request_summary: "recover safely".into(),
        conversation_snapshot: "full durable parent history".into(),
        conversation_summary: "parent was validating Phase 4".into(),
        work_stage_and_plan: "Phase 4 recovery".into(),
        memory_space_id: None,
        command: "cargo".into(),
        args: vec!["test".into()],
        cwd: cwd.into(),
        tool_call_id: "shell-call".into(),
        shell_log_start: 10,
        suggestion_cache_path: PathBuf::from("/tmp/test-suggestions.json"),
    }
}

fn save_handoff(store: &FilesystemHandoffStore, id: &str, state: HandoffState, cwd: &str) {
    let requested = RequestedShellExec {
        command: "cargo".into(),
        args: vec!["test".into(), "-j".into(), "1".into()],
        cwd: Some(cwd.into()),
        tool_call_id: Some("shell-call".into()),
    };
    let child_goal = ChildGoalMeta {
        id: format!("child-{id}"),
        handoff_id: id.into(),
        parent_goal_id: Some("parent-goal".into()),
        work_id: None,
        auto_root_work_id: None,
        close_reason: None,
        close_state: None,
        achievement: ChildGoalAchievement::Unknown,
    };
    store
        .save_handoff(&Handoff {
            id: id.into(),
            schema_version: HANDOFF_SCHEMA_VERSION,
            parent_task_id: "parent-task".into(),
            parent_conversation_id: "parent-conversation".into(),
            parent_run_id: "parent-run".into(),
            parent_goal_id: Some("parent-goal".into()),
            child_goal_id: child_goal.id.clone(),
            side_conversation_id: Some("side-conversation".into()),
            state,
            initial_cwd: cwd.into(),
            final_shell_cwd: None,
            parent_request_summary: format!("recover {id}"),
            requested_shell_execs: vec![requested.clone()],
            pending_human_request: Some("inspect the failing test".into()),
            conversation_snapshot_ref: "checkpoint.json#conversation_snapshot".into(),
            conversation_summary: "parent was validating Phase 4".into(),
            checkpoint_ref: "checkpoint.json".into(),
            before_observation_ref: "before".into(),
            after_observation_ref: None,
            shell_log_start: 10,
            shell_log_end: None,
            shell_generation: 1,
            return_reason: None,
            human_shell_exit_code: None,
            resume_error: None,
            created_at_ms: 1,
            updated_at_ms: 10,
        })
        .unwrap();
    store
        .save_checkpoint(
            id,
            &HandoffCheckpoint {
                parent_task_id: "parent-task".into(),
                parent_conversation_id: "parent-conversation".into(),
                parent_run_id: "parent-run".into(),
                pending_shell_exec: requested,
                parent_goal: "finish Phase 4 safely".into(),
                child_goal,
                conversation_snapshot: "full durable parent history".into(),
                conversation_summary: "parent was validating Phase 4".into(),
                cwd: cwd.into(),
                environment_metadata: serde_json::json!({
                    "handoff_host_id": "test-host",
                    "handoff_uid": 1000,
                })
                .to_string(),
                handoff_id: id.into(),
                side_conversation_id: Some("side-conversation".into()),
                command_candidates: Vec::new(),
                shell_log_start: 10,
                control_state: state,
                provider_metadata: None,
                tool_executions: vec![RecoverableToolExecution {
                    tool_call_id: "running-tool".into(),
                    tool_name: "shell_exec".into(),
                    status: RecoverableToolStatus::Running,
                }],
            },
        )
        .unwrap();
    store
        .append_shell_session(
            id,
            &ShellSessionIssueRequest {
                generation: 1,
                token_plaintext: "old-token".into(),
                now_ms: 1,
            },
        )
        .unwrap();
}

#[test]
fn abnormal_shell_exit_marks_handoff_orphaned() {
    let fixture = Fixture::new("existing", HandoffState::Completed);
    let runtime = Runtime {
        now: 20,
        owner_alive: false,
    };
    let launcher = Launcher {
        requests: Mutex::new(Vec::new()),
        result: Err(HumanShellLaunchError::MissingReturnMarker),
    };
    let policy = CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &fixture.store,
        &launcher,
        &Observer,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &NoopCollaborativeChildGoalService,
        &runtime,
    );
    let error = policy.intercept(parent_request(&fixture.cwd)).unwrap_err();
    assert!(matches!(
        error,
        ai::application::CollaborativeHandoffError::Launch(
            HumanShellLaunchError::MissingReturnMarker
        )
    ));
    let handoff = fixture.store.load_handoff("handoff-test").unwrap();
    assert_eq!(handoff.state, HandoffState::Orphaned);
    assert_eq!(
        handoff.return_reason.as_deref(),
        Some("abnormal_shell_exit")
    );
}

#[test]
fn parent_process_loss_marks_handoff_orphaned() {
    let fixture = Fixture::new("parent-loss", HandoffState::HumanActive);
    fixture.lease("parent-loss", 1, 10_000);
    let reconciled = ReconcileStaleHandoffs::new(
        &fixture.store,
        &Runtime {
            now: 20,
            owner_alive: false,
        },
    )
    .execute()
    .unwrap();
    assert_eq!(reconciled, vec!["parent-loss"]);
    assert_eq!(
        fixture.store.load_handoff("parent-loss").unwrap().state,
        HandoffState::Orphaned
    );
}

#[test]
fn resume_orphaned_spawns_new_shell_with_rotated_token() {
    let fixture = Fixture::new("resume-shell", HandoffState::Orphaned);
    let launcher = Launcher::returning(&fixture.cwd);
    ResumeOrphanedHandoff::new(
        &fixture.store,
        &launcher,
        &Runtime {
            now: 20,
            owner_alive: true,
        },
        &NoopCollaborativeChildGoalService,
    )
    .execute("resume-shell", &owner())
    .unwrap();
    let requests = launcher.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].context_version, 2);
    assert_eq!(requests[0].token, "rotated-token");
    assert_eq!(
        fixture.store.load_handoff("resume-shell").unwrap().state,
        HandoffState::Returned
    );
}

#[test]
fn resume_rotates_token_and_rejects_old_generation() {
    let fixture = Fixture::new("rotate", HandoffState::Orphaned);
    let launcher = Launcher::returning(&fixture.cwd);
    ResumeOrphanedHandoff::new(
        &fixture.store,
        &launcher,
        &Runtime {
            now: 20,
            owner_alive: true,
        },
        &NoopCollaborativeChildGoalService,
    )
    .execute("rotate", &owner())
    .unwrap();
    let sessions = fixture.store.list_shell_sessions("rotate").unwrap();
    assert!(!validate_shell_token(&sessions, "old-token", 1));
    assert!(validate_shell_token(&sessions, "rotated-token", 2));
}

#[test]
fn second_resume_rejected_while_lease_active() {
    let fixture = Fixture::new("exclusive", HandoffState::Orphaned);
    fixture.lease("exclusive", 10, 1_000);
    let error = ResumeOrphanedHandoff::new(
        &fixture.store,
        &Launcher::returning(&fixture.cwd),
        &Runtime {
            now: 20,
            owner_alive: true,
        },
        &NoopCollaborativeChildGoalService,
    )
    .execute("exclusive", &owner())
    .unwrap_err();
    assert!(matches!(
        error,
        CollaborativeRecoveryError::Store(ai::ports::outbound::HandoffStoreError::LeaseConflict)
    ));
}

#[test]
fn resume_lists_multiple_recoverable_handoffs() {
    let fixture = Fixture::new("first", HandoffState::Orphaned);
    save_handoff(
        &fixture.store,
        "second",
        HandoffState::Returned,
        &fixture.cwd,
    );
    let runtime = Runtime {
        now: 20,
        owner_alive: true,
    };
    let list = list_recoverable_handoffs(&fixture.store, runtime.now_ms()).unwrap();
    assert_eq!(list.len(), 2);
    assert!(matches!(
        select_recoverable_handoff(&fixture.store, None, runtime.now_ms()),
        Err(CollaborativeRecoveryError::HandoffIdRequired(items)) if items.len() == 2
    ));
}

#[test]
fn resume_returned_restarts_parent_without_shell() {
    let fixture = Fixture::new("returned", HandoffState::Returned);
    let launcher = Launcher::returning(&fixture.cwd);
    let runtime = Runtime {
        now: 20,
        owner_alive: true,
    };
    let context = ResumeReturnedParent::new(&fixture.store, &Observer, &runtime)
        .prepare("returned", &owner())
        .unwrap();
    assert_eq!(context.parent_conversation_id, "parent-conversation");
    assert!(launcher.requests.lock().unwrap().is_empty());
}

#[test]
fn resumed_parent_run_carries_pending_shell_exec_context() {
    let fixture = Fixture::new("semantic", HandoffState::Returned);
    let context = ResumeReturnedParent::new(
        &fixture.store,
        &Observer,
        &Runtime {
            now: 20,
            owner_alive: true,
        },
    )
    .prepare("semantic", &owner())
    .unwrap();
    let prompt: serde_json::Value = serde_json::from_str(&context.semantic_prompt()).unwrap();
    assert_eq!(prompt["pending_shell_exec"]["command"], "cargo");
    assert_eq!(prompt["handoff_id"], "semantic");
}

#[test]
fn recovery_does_not_auto_rerun_unknown_tools() {
    let fixture = Fixture::new("unknown", HandoffState::Returned);
    let context = ResumeReturnedParent::new(
        &fixture.store,
        &Observer,
        &Runtime {
            now: 20,
            owner_alive: true,
        },
    )
    .prepare("unknown", &owner())
    .unwrap();
    assert_eq!(context.uncertain_tool_executions, vec!["running-tool"]);
    assert!(context
        .semantic_prompt()
        .contains("Do not automatically re-run UNKNOWN"));
    assert!(has_unknown_tools(
        &fixture.store.load_checkpoint("unknown").unwrap()
    ));
}

#[test]
fn side_agent_crash_marks_running_tools_unknown() {
    let fixture = Fixture::new("side-crash", HandoffState::SideAgentRunning);
    MarkOrphaned::new(
        &fixture.store,
        &Runtime {
            now: 20,
            owner_alive: false,
        },
    )
    .execute("side-crash", "side_agent_crash")
    .unwrap();
    assert!(has_unknown_tools(
        &fixture.store.load_checkpoint("side-crash").unwrap()
    ));
}

#[test]
fn resume_preserves_pending_human_request_state() {
    let fixture = Fixture::new("waiting", HandoffState::Orphaned);
    ResumeOrphanedHandoff::new(
        &fixture.store,
        &Launcher::returning(&fixture.cwd),
        &Runtime {
            now: 20,
            owner_alive: true,
        },
        &NoopCollaborativeChildGoalService,
    )
    .execute("waiting", &owner())
    .unwrap();
    assert_eq!(
        fixture
            .store
            .load_handoff("waiting")
            .unwrap()
            .pending_human_request
            .as_deref(),
        Some("inspect the failing test")
    );
}

#[test]
fn handoff_cancelled_when_shell_never_started_after_checkpoint() {
    let fixture = Fixture::new("existing", HandoffState::Completed);
    let runtime = Runtime {
        now: 20,
        owner_alive: false,
    };
    let launcher = Launcher {
        requests: Mutex::new(Vec::new()),
        result: Err(HumanShellLaunchError::Failed("fault: exec failed".into())),
    };
    CollaborativeShellExecPolicy::new(
        CollaborativeExecutionContext::parent_enabled(),
        &fixture.store,
        &launcher,
        &Observer,
        &NoopParentToolBarrier,
        &NoopHandoffCandidatePublisher,
        &NoopCollaborativeChildGoalService,
        &runtime,
    )
    .intercept(parent_request(&fixture.cwd))
    .unwrap_err();
    assert_eq!(
        fixture.store.load_handoff("handoff-test").unwrap().state,
        HandoffState::Cancelled
    );
}

#[test]
fn parent_resume_failure_returns_to_returned_state() {
    let fixture = Fixture::new("parent-fail", HandoffState::Returned);
    let runtime = Runtime {
        now: 20,
        owner_alive: true,
    };
    let service = ResumeReturnedParent::new(&fixture.store, &Observer, &runtime);
    service.prepare("parent-fail", &owner()).unwrap();
    service
        .finish("parent-fail", Err("aibe unavailable".into()))
        .unwrap();
    let handoff = fixture.store.load_handoff("parent-fail").unwrap();
    assert_eq!(handoff.state, HandoffState::Returned);
    assert_eq!(handoff.resume_error.as_deref(), Some("aibe unavailable"));
}

#[test]
fn orphaned_handoff_keeps_child_goal_open() {
    let fixture = Fixture::new("goal-open", HandoffState::HumanActive);
    MarkOrphaned::new(
        &fixture.store,
        &Runtime {
            now: 20,
            owner_alive: false,
        },
    )
    .execute("goal-open", "owner_lost")
    .unwrap();
    let checkpoint = fixture.store.load_checkpoint("goal-open").unwrap();
    assert!(checkpoint.child_goal.close_reason.is_none());
    assert_eq!(
        checkpoint.child_goal.achievement,
        ChildGoalAchievement::Unknown
    );
}

#[test]
fn ctrl_d_during_side_run_returns_to_parent() {
    let fixture = Fixture::new("ctrl-d", HandoffState::SideAgentRunning);
    ReturnControlFromShell::new(
        &fixture.store,
        &Runtime {
            now: 20,
            owner_alive: true,
        },
        &NoopCollaborativeChildGoalService,
    )
    .execute(
        "ctrl-d",
        &HumanShellReturn {
            normal_return: true,
            exit_code: Some(0),
            final_cwd: fixture.cwd.clone().into(),
            shell_session_id: "test-session".into(),
            shell_session_dir: fixture.cwd.clone().into(),
            shell_log_start: 0,
            shell_log_end: 1,
        },
    )
    .unwrap();
    assert_eq!(
        fixture.store.load_handoff("ctrl-d").unwrap().state,
        HandoffState::Returned
    );
    assert!(
        fixture.store.load_lease("ctrl-d").unwrap().is_none(),
        "ReturnControlFromShell must release human-shell lease"
    );
    assert!(has_unknown_tools(
        &fixture.store.load_checkpoint("ctrl-d").unwrap()
    ));
}

#[test]
fn resuming_parent_owner_loss_returns_to_returned() {
    let fixture = Fixture::new("resume-crash", HandoffState::ResumingParent);
    fixture.lease("resume-crash", 1, 120_000);
    let reconciled = ReconcileStaleHandoffs::new(
        &fixture.store,
        &Runtime {
            now: 20,
            owner_alive: false,
        },
    )
    .execute()
    .unwrap();
    assert_eq!(reconciled, vec!["resume-crash"]);
    let handoff = fixture.store.load_handoff("resume-crash").unwrap();
    assert_eq!(handoff.state, HandoffState::Returned);
    assert_eq!(
        handoff.resume_error.as_deref(),
        Some("lease_owner_process_lost")
    );
}

#[test]
fn lease_expiry_alone_does_not_auto_resume_parent() {
    let fixture = Fixture::new("expired", HandoffState::HumanActive);
    fixture.lease("expired", 1, 5);
    let reconciled = ReconcileStaleHandoffs::new(
        &fixture.store,
        &Runtime {
            now: 20,
            owner_alive: true,
        },
    )
    .execute()
    .unwrap();
    assert!(reconciled.is_empty());
    assert_eq!(
        fixture.store.load_handoff("expired").unwrap().state,
        HandoffState::HumanActive
    );
}

#[test]
fn parent_resume_tool_lifecycle_syncs_completed_tools() {
    let fixture = Fixture::new("resume-tools", HandoffState::Returned);
    let runtime = Runtime {
        now: 2,
        owner_alive: true,
    };
    ResumeReturnedParent::new(&fixture.store, &Observer, &runtime)
        .prepare("resume-tools", &owner())
        .unwrap();
    record_handoff_tool_running(
        &fixture.store,
        "resume-tools",
        "parent-tool-1",
        "file_write",
    )
    .unwrap();
    finalize_parent_resume_tool_tracking(
        &fixture.store,
        &ResumedHandoffSync {
            handoff_id: "resume-tools".into(),
            sync_start_tool_call_id: Some("parent-tool-1".into()),
            sync_end_before_tool_call_id: None,
        },
        true,
        Some(&[aibe_protocol::ExecutedToolCall::ok(
            "parent-tool-1".to_string(),
            "file_write".to_string(),
            serde_json::json!({"path": "README.md"}),
            "written".to_string(),
        )]),
    )
    .unwrap();
    let checkpoint = fixture.store.load_checkpoint("resume-tools").unwrap();
    let tool = checkpoint
        .tool_executions
        .iter()
        .find(|tool| tool.tool_call_id == "parent-tool-1")
        .expect("parent resume tool");
    assert_eq!(tool.status, RecoverableToolStatus::Completed);
    ResumeReturnedParent::new(&fixture.store, &Observer, &runtime)
        .finish("resume-tools", Ok(()))
        .unwrap();
}
