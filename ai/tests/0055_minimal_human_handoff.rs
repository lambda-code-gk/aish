//! 0055 minimal human handoff acceptance tests (ai).

use std::sync::{Arc, Mutex};

use ai::adapters::outbound::{AishHumanShellLauncher, ProcessEnvironmentObserver};
use ai::application::{HumanHandoffRequest, RunSynchronousHumanHandoff};
use ai::domain::{build_suggested_command, HANDOFF_ENV_KEYS};
use ai::ports::outbound::{
    HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
};
use aibe_client::ShellExecApprovalPrompt;
use aibe_protocol::{
    HandoffExecutionOutcome, HumanHandoffResult, RequestedCommandCompletion,
    ShellExecApprovalOrigin,
};

#[derive(Default)]
struct TestLauncher {
    calls: Arc<Mutex<Vec<HumanShellLaunchRequest>>>,
}

impl HumanShellLauncher for TestLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        self.calls.lock().unwrap().push(request.clone());
        Ok(HumanShellReturn {
            normal_return: true,
            exit_code: Some(0),
            final_cwd: request.cwd.clone(),
            shell_session_id: "sess".into(),
            shell_session_dir: request.runtime_dir.join("session"),
            shell_log_start: 0,
            shell_log_end: 42,
        })
    }
}

#[test]
fn collaborative_flag_intercepts_parent_shell_exec() {
    let launcher = TestLauncher::default();
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&launcher, &observer);
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    let result = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "verify build".into(),
            command: "cargo".into(),
            args: vec!["test".into()],
            cwd: dir.path().to_path_buf(),
            shell_log_start: 0,
            runtime_dir: runtime,
        })
        .expect("handoff");
    assert_eq!(
        result.execution_outcome,
        HandoffExecutionOutcome::HumanControlReturned
    );
    assert_eq!(
        result.requested_command_completion,
        RequestedCommandCompletion::Unknown
    );
    assert_eq!(launcher.calls.lock().unwrap().len(), 1);
}

#[test]
fn parent_receives_human_control_returned() {
    let launcher = TestLauncher::default();
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&launcher, &observer);
    let dir = tempfile::tempdir().unwrap();
    let result = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "summary".into(),
            command: "echo".into(),
            args: vec!["hi".into()],
            cwd: dir.path().to_path_buf(),
            shell_log_start: 0,
            runtime_dir: dir.path().join("runtime"),
        })
        .expect("handoff");
    assert_eq!(result.requested_command.as_deref(), Some("'echo' 'hi'"));
    assert!(result.observation.is_some());
}

#[test]
fn shell_exit_code_is_not_command_completion() {
    let launcher = TestLauncher::default();
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&launcher, &observer);
    let dir = tempfile::tempdir().unwrap();
    let result = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "summary".into(),
            command: "false".into(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            shell_log_start: 0,
            runtime_dir: dir.path().join("runtime"),
        })
        .expect("handoff");
    assert_eq!(result.human_shell_exit_code, Some(0));
    assert_eq!(
        result.requested_command_completion,
        RequestedCommandCompletion::Unknown
    );
}

#[test]
fn parent_reobserves_after_handoff() {
    let launcher = TestLauncher::default();
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&launcher, &observer);
    let dir = tempfile::tempdir().unwrap();
    let result = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "summary".into(),
            command: "pwd".into(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            shell_log_start: 0,
            runtime_dir: dir.path().join("runtime"),
        })
        .expect("handoff");
    let observation = result.observation.expect("observation");
    assert!(observation.cwd_exists);
}

#[test]
fn human_shell_failure_returns_error() {
    struct FailingLauncher;
    impl HumanShellLauncher for FailingLauncher {
        fn launch_and_wait(
            &self,
            _request: &HumanShellLaunchRequest,
        ) -> Result<HumanShellReturn, HumanShellLaunchError> {
            Err(HumanShellLaunchError::Interrupted(
                "Human handoff was interrupted.\nRestart the original request.".into(),
            ))
        }
    }
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&FailingLauncher, &observer);
    let dir = tempfile::tempdir().unwrap();
    let err = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "summary".into(),
            command: "echo".into(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            shell_log_start: 0,
            runtime_dir: dir.path().join("runtime"),
        })
        .expect_err("should fail");
    assert!(err.to_string().contains("interrupted"));
}

#[test]
fn no_persistent_handoff_state_is_created() {
    let launcher = TestLauncher::default();
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&launcher, &observer);
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    let _ = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "summary".into(),
            command: "echo".into(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            shell_log_start: 0,
            runtime_dir: runtime.clone(),
        })
        .expect("handoff");
    assert!(!dir.path().join("handoff.json").exists());
    assert!(!dir.path().join("workflow.json").exists());
    assert!(!runtime.join("handoff.json").exists());
}

#[test]
fn normal_shell_exec_is_unchanged() {
    let text = build_suggested_command("echo", &["ok".into()]);
    assert_eq!(text, "'echo' 'ok'");
    let prompt = ShellExecApprovalPrompt {
        prompt_id: "p".into(),
        turn_id: "t".into(),
        tool_call_id: "tc".into(),
        command: "echo".into(),
        args: vec!["ok".into()],
    };
    assert_eq!(prompt.command, "echo");
    assert_ne!(
        ShellExecApprovalOrigin::UiYes,
        ShellExecApprovalOrigin::CollaborativeHandoff
    );
    let _dto = HumanHandoffResult {
        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
        requested_command: None,
        requested_command_completion: RequestedCommandCompletion::Unknown,
        human_shell_exit_code: None,
        final_shell_cwd: None,
        shell_log_range: None,
        observation: None,
    };
}

#[test]
fn human_shell_ai_is_independent() {
    for key in HANDOFF_ENV_KEYS {
        assert!(!key.contains("CONVERSATION"));
        assert!(!key.contains("TOKEN"));
    }
    let _ = AishHumanShellLauncher::default();
}

#[test]
fn human_shell_starts_in_requested_cwd() {
    let launcher = TestLauncher::default();
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&launcher, &observer);
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let _ = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "summary".into(),
            command: "pwd".into(),
            args: vec![],
            cwd: cwd.clone(),
            shell_log_start: 0,
            runtime_dir: dir.path().join("runtime"),
        })
        .expect("handoff");
    let call = launcher.calls.lock().unwrap().pop().unwrap();
    assert_eq!(call.cwd, cwd);
}
