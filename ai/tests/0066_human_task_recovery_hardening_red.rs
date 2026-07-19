//! 0066 acceptance tests for manual Human Task recovery hardening.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ai::adapters::outbound::{HumanTaskFileStore, SystemHumanTaskTimeFormatter};
use ai::application::{HumanTaskRecover, HumanTaskRecoverError, HumanTaskResume, HumanTaskStatus};
use ai::domain::human_task_checkpoint::*;
use ai::ports::outbound::*;
use aibe_protocol::{
    HandoffExecutionOutcome, HumanTaskRequest, HumanTaskResult, PostHandoffObservation,
    ShellLogRange,
};

fn running_checkpoint(cwd: PathBuf) -> HumanTaskCheckpointV1 {
    HumanTaskCheckpointV1 {
        version: 1,
        task_id: HumanTaskId::parse("ht-20260718-112233").unwrap(),
        state: HumanTaskWorkflowState::Running,
        task: HumanTaskRequest {
            objective: "recover deployment review".into(),
            reason: None,
            instructions: vec!["inspect".into()],
            suggested_commands: vec!["git status".into()],
            completion_criteria: vec!["report".into()],
        },
        parent: HumanTaskParentContext {
            ai_session_id: "session".into(),
            conversation_id: "conversation".into(),
            turn_id: "turn".into(),
            user_request: "deploy safely".into(),
            original_cwd: cwd.clone(),
            llm_profile: "default".into(),
        },
        created_at_ms: 10,
        updated_at_ms: 10,
        suspended_at_ms: None,
        suspend_reason: None,
        current_cwd: cwd,
        segments: vec![],
        final_result: None,
        continuation: HumanTaskContinuationState::default(),
    }
}

fn continuing_checkpoint(cwd: PathBuf) -> HumanTaskCheckpointV1 {
    let mut checkpoint = running_checkpoint(cwd.clone());
    let observation = PostHandoffObservation {
        cwd_exists: true,
        cwd: cwd.display().to_string(),
        git_head: None,
        git_branch: None,
        git_status: None,
        shell_log_tail: None,
        shell_log_truncated: None,
        observation_errors: vec![],
        human_task_evidence: None,
    };
    let range = ShellLogRange {
        start: 1,
        end: Some(2),
    };
    checkpoint.state = HumanTaskWorkflowState::Continuing;
    checkpoint.updated_at_ms = 20;
    checkpoint.segments.push(HumanShellSegment {
        index: 0,
        shell_session_id: "shell".into(),
        started_at_ms: 10,
        ended_at_ms: 20,
        initial_cwd: cwd.clone(),
        final_cwd: cwd.clone(),
        shell_log_range: range.clone(),
        observation: observation.clone(),
        end_reason: HumanShellSegmentEnd::Done,
    });
    checkpoint.final_result = Some(HumanTaskResult {
        status: HandoffExecutionOutcome::Done,
        task: checkpoint.task.clone(),
        verified: false,
        human_shell_exit_code: Some(0),
        final_shell_cwd: Some(cwd.display().to_string()),
        shell_log_range: Some(range),
        observation: Some(observation),
        error: None,
        task_id: None,
        suspend_reason: None,
    });
    checkpoint.continuation.continuation_turn_id = Some("stable-continuation".into());
    checkpoint
}

#[test]
fn human_task_recovery_vertical_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store.save(&running_checkpoint(dir.path().into())).unwrap();

    let output = HumanTaskRecover::new(&store, 30)
        .execute(false, |_| true)
        .unwrap();
    assert!(output.contains("ai human-task resume"));
    let recovered = store.load_active().unwrap();
    assert_eq!(recovered.state, HumanTaskWorkflowState::Suspended);
    assert_eq!(
        recovered.suspend_reason.as_deref(),
        Some("unexpected_process_termination")
    );
    assert!(recovered.validate().is_ok());

    struct Identity;
    impl HumanTaskIdentity for Identity {
        fn new_task_id(&self) -> HumanTaskId {
            unreachable!()
        }
        fn now_ms(&self) -> u64 {
            40
        }
    }
    struct Launcher;
    impl HumanShellLauncher for Launcher {
        fn launch_and_wait(
            &self,
            request: &HumanShellLaunchRequest,
            _: &AtomicBool,
        ) -> Result<HumanShellReturn, HumanShellLaunchError> {
            Err(HumanShellLaunchError::Suspended {
                returned: Box::new(HumanShellReturn {
                    outcome: HumanShellOutcome::Suspended,
                    suspend_reason: Some("later".into()),
                    exit_code: Some(0),
                    final_cwd: request.cwd.clone(),
                    shell_session_id: "resumed-shell".into(),
                    shell_session_dir: PathBuf::new(),
                    shell_log_start: 1,
                    shell_log_end: 2,
                }),
                reason: Some("later".into()),
            })
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
            PostHandoffObservation {
                cwd_exists: true,
                cwd: cwd.display().to_string(),
                git_head: None,
                git_branch: None,
                git_status: None,
                shell_log_tail: None,
                shell_log_truncated: None,
                observation_errors: vec![],
                human_task_evidence: None,
            }
        }
    }
    let resumed = HumanTaskResume::new(&store, &Identity, &Launcher, &Observer)
        .execute(None, dir.path().join("runtime"), &AtomicBool::new(false))
        .unwrap();
    assert!(resumed.contains("Human Task suspended"));
    assert_eq!(store.load_active().unwrap().segments.len(), 1);
}

#[test]
fn human_task_recovery_continuing_to_result_pending() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&continuing_checkpoint(dir.path().into()))
        .unwrap();
    HumanTaskRecover::new(&store, 30)
        .execute(false, |_| true)
        .unwrap();
    let recovered = store.load_active().unwrap();
    assert_eq!(recovered.state, HumanTaskWorkflowState::ResultPending);
    assert_eq!(
        recovered.continuation.continuation_turn_id.as_deref(),
        Some("stable-continuation")
    );
}

#[test]
fn human_task_recovery_status_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store.save(&running_checkpoint(dir.path().into())).unwrap();
    let running = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(running.contains("ai human-task recover"));

    store
        .save(&continuing_checkpoint(dir.path().into()))
        .unwrap();
    let continuing = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(continuing.contains("ai human-task recover"));

    let mut finished = continuing_checkpoint(dir.path().into());
    finished.state = HumanTaskWorkflowState::Finished;
    store.save(&finished).unwrap();
    let finished = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(finished.contains("ai human-task cancel --yes"));
    assert!(!finished.contains("ai human-task recover\n"));
}

fn write_invalid_residue(history: &Path, contents: Option<&[u8]>, mode: u32) -> PathBuf {
    let task_dir = history.join("human-tasks/ht-20260718-112233");
    fs::create_dir_all(&task_dir).unwrap();
    fs::set_permissions(
        history.join("human-tasks"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::set_permissions(&task_dir, fs::Permissions::from_mode(0o700)).unwrap();
    if let Some(contents) = contents {
        let path = task_dir.join("checkpoint.json");
        fs::write(&path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
    task_dir
}

#[test]
fn human_task_recovery_force_invalid_cleanup() {
    for (contents, mode) in [
        (Some(b"not-json".as_slice()), 0o600),
        (Some(b"{}".as_slice()), 0o644),
        (None, 0o600),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let task_dir = write_invalid_residue(dir.path(), contents, mode);
        let store = HumanTaskFileStore::new(dir.path().into());
        assert!(HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
            .render()
            .is_err());
        assert_eq!(
            HumanTaskRecover::new(&store, 30).execute(false, |_| true),
            Err(HumanTaskRecoverError::ForceInvalidRequired)
        );
        assert!(task_dir.exists());
        HumanTaskRecover::new(&store, 30)
            .execute(true, |_| true)
            .unwrap();
        assert!(!task_dir.exists());
    }
    let dir = tempfile::tempdir().unwrap();
    let task_dir = write_invalid_residue(dir.path(), Some(b"not-json"), 0o600);
    fs::set_permissions(&task_dir, fs::Permissions::from_mode(0o000)).unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    HumanTaskRecover::new(&store, 30)
        .execute(true, |_| true)
        .unwrap();
    assert!(!task_dir.exists());
}

#[test]
fn human_task_recovery_force_invalid_cleanup_does_not_follow_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("keep-me.txt");
    fs::write(&outside_file, b"secret").unwrap();
    let root = dir.path().join("human-tasks");
    fs::create_dir_all(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let link = root.join("ht-20260718-abcdef");
    symlink(outside.path(), &link).unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    HumanTaskRecover::new(&store, 30)
        .execute(true, |_| true)
        .unwrap();
    assert!(!link.exists());
    assert!(outside_file.exists());
    assert_eq!(fs::read(&outside_file).unwrap(), b"secret");
}

#[test]
fn human_task_recovery_is_confirmed_and_locked() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store.save(&running_checkpoint(dir.path().into())).unwrap();
    assert_eq!(
        HumanTaskRecover::new(&store, 30).execute(false, |_| false),
        Err(HumanTaskRecoverError::NotConfirmed)
    );
    assert_eq!(
        store.load_active().unwrap().state,
        HumanTaskWorkflowState::Running
    );
    let _lock = store.lock_exclusive().unwrap();
    assert_eq!(
        HumanTaskRecover::new(&store, 30).execute(false, |_| true),
        Err(HumanTaskRecoverError::Busy)
    );
}

#[test]
fn human_task_recovery_preserves_existing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let mut suspended = running_checkpoint(dir.path().into());
    suspended.state = HumanTaskWorkflowState::Suspended;
    suspended.updated_at_ms = 30;
    suspended.suspended_at_ms = Some(30);
    suspended.suspend_reason = Some("unexpected_process_termination".into());
    store.save(&suspended).unwrap();
    assert_eq!(
        HumanTaskRecover::new(&store, 40).execute(false, |_| true),
        Err(HumanTaskRecoverError::AlreadyRecoverable)
    );
    assert_eq!(store.load_active().unwrap(), suspended);
}
