// 0055 Collaborative Human Handoff acceptance tests.
// Phase 1 tests are active; later phases remain #[ignore] until implemented.

use ai::adapters::outbound::FilesystemHandoffStore;
use ai::domain::{
    build_candidate_command, checkpoint_has_required_fields, checkpoint_serialized_field_names,
    close_child_goal_on_control_returned, should_close_child_goal, try_transition,
    validate_shell_token, ChildGoalAchievement, ChildGoalCloseReason, ChildGoalMeta,
    CommandCandidate, CommandCandidateSource, Handoff, HandoffCheckpoint, HandoffEvent,
    HandoffShellSession, HandoffState, RequestedShellExec, CHECKPOINT_REQUIRED_FIELD_NAMES,
    HANDOFF_SCHEMA_VERSION,
};
use ai::ports::outbound::{
    CheckpointRepository, CommandCandidateStore, HandoffRepository, HandoffShellSessionStore,
    HandoffStoreError, LeaseAcquireRequest, LeaseRepository, ShellSessionIssueRequest,
};

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
            close_reason: None,
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
        close_reason: None,
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
#[ignore = "0055 phase2: checkpoint_persisted_before_human_shell_spawn"]
fn checkpoint_persisted_before_human_shell_spawn() {
    panic!("0055 phase2 not implemented");
}

#[test]
#[ignore = "0055 phase2: collaborative_flag_enables_parent_policy"]
fn collaborative_flag_enables_parent_policy() {
    panic!("0055 phase2 not implemented");
}

#[test]
#[ignore = "0055 phase2: handoff_completes_normal_parent_resume_flow"]
fn handoff_completes_normal_parent_resume_flow() {
    panic!("0055 phase2 not implemented");
}

#[test]
#[ignore = "0055 phase2: handoff_records_final_shell_cwd_on_return"]
fn handoff_records_final_shell_cwd_on_return() {
    panic!("0055 phase2 not implemented");
}

#[test]
#[ignore = "0055 phase2: missing_cwd_rejects_human_shell_spawn"]
fn missing_cwd_rejects_human_shell_spawn() {
    panic!("0055 phase2 not implemented");
}

#[test]
#[ignore = "0055 phase2: non_parent_role_skips_handoff"]
fn non_parent_role_skips_handoff() {
    panic!("0055 phase2 not implemented");
}

#[test]
#[ignore = "0055 phase5: collaborative_audit_events_are_emitted"]
fn collaborative_audit_events_are_emitted() {
    panic!("0055 phase5 not implemented");
}

#[test]
#[ignore = "0055 phase5: collaborative_config_defaults_match_spec"]
fn collaborative_config_defaults_match_spec() {
    panic!("0055 phase5 not implemented");
}

#[test]
#[ignore = "0055 phase5: handoff_token_not_in_replay_output"]
fn handoff_token_not_in_replay_output() {
    panic!("0055 phase5 not implemented");
}

#[test]
#[ignore = "0055 phase5: handoff_token_redacted_from_shell_log"]
fn handoff_token_redacted_from_shell_log() {
    panic!("0055 phase5 not implemented");
}
