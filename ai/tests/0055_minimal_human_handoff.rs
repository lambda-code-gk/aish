//! 0055 minimal human handoff acceptance tests (ai).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ai::adapters::outbound::ProcessEnvironmentObserver;
use ai::application::{HumanHandoffRequest, RunSynchronousHumanHandoff};
use ai::domain::HANDOFF_ENV_KEYS;
use ai::ports::outbound::{
    HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
};
use aibe_protocol::{HandoffExecutionOutcome, RequestedCommandCompletion};

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
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("human_session");
    std::fs::create_dir_all(&session_dir).unwrap();
    let log_path = session_dir.join("log.jsonl");
    std::fs::write(
        &log_path,
        b"{\"event\":\"command_start\"}\n{\"event\":\"stdout\",\"data\":\"ran ok\\n\"}\n",
    )
    .unwrap();
    let log_end = std::fs::metadata(&log_path).unwrap().len();

    struct SessionLauncher {
        session_dir: PathBuf,
        log_end: u64,
    }

    impl HumanShellLauncher for SessionLauncher {
        fn launch_and_wait(
            &self,
            request: &HumanShellLaunchRequest,
        ) -> Result<HumanShellReturn, HumanShellLaunchError> {
            Ok(HumanShellReturn {
                normal_return: true,
                exit_code: Some(0),
                final_cwd: request.cwd.clone(),
                shell_session_id: "sess".into(),
                shell_session_dir: self.session_dir.clone(),
                shell_log_start: 0,
                shell_log_end: self.log_end,
            })
        }
    }

    let launcher = SessionLauncher {
        session_dir: session_dir.clone(),
        log_end,
    };
    let observer = ProcessEnvironmentObserver::default();
    let service = RunSynchronousHumanHandoff::new(&launcher, &observer);
    let result = service
        .execute(HumanHandoffRequest {
            parent_request_summary: "summary".into(),
            command: "pwd".into(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            runtime_dir: dir.path().join("runtime"),
        })
        .expect("handoff");
    let observation = result.observation.expect("observation");
    assert!(observation.cwd_exists);
    let tail = observation
        .shell_log_tail
        .expect("human shell transcript must be included");
    assert!(
        tail.contains("ran ok"),
        "observation must use human shell log offsets: {tail}"
    );
    let range = result.shell_log_range.expect("range");
    assert_eq!(range.start, 0);
    assert_eq!(range.end, Some(log_end));
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
            runtime_dir: dir.path().join("runtime"),
        })
        .expect_err("should fail");
    assert!(err.to_string().contains("interrupted"));
}

#[test]
fn human_shell_ai_is_independent() {
    for key in HANDOFF_ENV_KEYS {
        assert!(!key.contains("CONVERSATION"));
        assert!(!key.contains("TOKEN"));
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
            runtime_dir: runtime,
        })
        .expect("handoff");
    assert_eq!(
        result.execution_outcome,
        HandoffExecutionOutcome::HumanControlReturned
    );
    assert_eq!(launcher.calls.lock().unwrap().len(), 1);
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
            runtime_dir: dir.path().join("runtime"),
        })
        .expect("handoff");
    let call = launcher.calls.lock().unwrap().pop().unwrap();
    assert_eq!(call.cwd, cwd);
}
