use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use ai::application::{plan_ask_launch_for_mode, ExecuteHumanTask};
use ai::clap_cli::{AiCli, AiCommand};
use ai::domain::{append_collaborative_instruction, ConfigToolsTokens, ExecutionMode};
use ai::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn,
};
use aibe_protocol::{
    HandoffExecutionOutcome, HumanHandoffFailure, HumanTaskEvidence, HumanTaskRequest,
    PostHandoffObservation,
};
use clap::{CommandFactory, Parser};

fn task() -> HumanTaskRequest {
    HumanTaskRequest {
        objective: "inspect workspace".into(),
        reason: Some("needs human judgment".into()),
        instructions: vec!["review changes".into()],
        completion_criteria: vec!["review complete".into()],
    }
}

#[derive(Default)]
struct FakeLauncher {
    request: Mutex<Option<HumanShellLaunchRequest>>,
}
impl HumanShellLauncher for FakeLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
        _: &AtomicBool,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        *self.request.lock().expect("request") = Some(request.clone());
        Ok(HumanShellReturn {
            normal_return: true,
            exit_code: Some(7),
            final_cwd: request.cwd.clone(),
            shell_session_id: "s".into(),
            shell_session_dir: PathBuf::new(),
            shell_log_start: 2,
            shell_log_end: 4,
        })
    }
}
struct FakeObserver;
impl EnvironmentObserver for FakeObserver {
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
            observation_errors: Vec::new(),
            human_task_evidence: Some(HumanTaskEvidence {
                commands: Vec::new(),
                truncated: false,
            }),
        }
    }
}

#[test]
fn collab_cli_selects_collaborative_mode() {
    assert!(matches!(
        AiCli::try_parse_from(["ai", "collab", "help me"])
            .unwrap()
            .command,
        AiCommand::Collab { .. }
    ));
    assert!(matches!(
        AiCli::try_parse_from(["ai", "ask", "help me"])
            .unwrap()
            .command,
        AiCommand::Ask { .. }
    ));
}

#[test]
fn collab_legacy_flag_maps_to_same_mode() {
    let AiCommand::Ask { turn, .. } = AiCli::try_parse_from(["ai", "ask", "--collaborative", "x"])
        .unwrap()
        .command
    else {
        panic!("ask")
    };
    assert_eq!(
        ExecutionMode::from_legacy_flag(turn.collaborative),
        ExecutionMode::Collaborative
    );
    assert_eq!(
        ExecutionMode::from_legacy_flag(false),
        ExecutionMode::Normal
    );
}

#[test]
fn collab_preserves_explicit_tools_without_exec_requirement() {
    let plan = plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        Some("read_file,grep"),
        "/tmp/x".into(),
        false,
        ExecutionMode::Collaborative,
    )
    .unwrap();
    let names: Vec<_> = plan
        .resolved_tools
        .allowlist
        .names()
        .iter()
        .map(|n| n.as_str())
        .collect();
    assert_eq!(names, ["read_file", "grep", "human_task"]);
    let none = plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        Some("none"),
        "/tmp/x".into(),
        false,
        ExecutionMode::Collaborative,
    )
    .unwrap();
    assert_eq!(
        none.resolved_tools.allowlist.names()[0].as_str(),
        "human_task"
    );
}

#[test]
fn human_task_is_published_only_in_collaborative_mode() {
    let normal = plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        None,
        "/tmp/x".into(),
        false,
        ExecutionMode::Normal,
    )
    .unwrap();
    assert!(normal.resolved_tools.allowlist.is_empty());
    assert!(plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        Some("human_task"),
        "/tmp/x".into(),
        false,
        ExecutionMode::Normal
    )
    .is_err());
}

#[test]
fn human_task_schema_matches_request_contract() {
    assert_eq!(
        HumanTaskRequest {
            objective: " x ".into(),
            reason: Some("  ".into()),
            instructions: vec![" y ".into()],
            completion_criteria: vec![]
        }
        .normalized()
        .unwrap()
        .objective,
        "x"
    );
    assert!(serde_json::from_value::<HumanTaskRequest>(
        serde_json::json!({"objective":"x","unknown":1})
    )
    .is_err());
    assert!(HumanTaskRequest {
        objective: "x".repeat(70_000),
        reason: None,
        instructions: vec![],
        completion_criteria: vec![]
    }
    .normalized()
    .is_err());
}

#[test]
fn human_task_briefing_uses_task_labels_and_omits_empty_sections() {
    let briefing = aibe_protocol::HumanTaskBriefing::from(&HumanTaskRequest {
        objective: "x".into(),
        reason: None,
        instructions: vec![],
        completion_criteria: vec![],
    });
    let json = serde_json::to_value(briefing).unwrap();
    assert_eq!(json["version"], 1);
    assert!(
        json.get("reason").is_none()
            && json.get("instructions").is_none()
            && json.get("completion_criteria").is_none()
    );
}

#[test]
fn execute_human_task_uses_existing_human_shell_ports() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = FakeLauncher::default();
    let result = ExecuteHumanTask::new(&launcher, &FakeObserver).execute(
        task(),
        dir.path().into(),
        dir.path().join("runtime"),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Done);
    let launch = launcher.request.lock().unwrap().clone().unwrap();
    assert_eq!(launch.task_briefing.unwrap().version, 1);
}

#[test]
fn human_task_result_reuses_status_and_observation_types() {
    let dir = tempfile::tempdir().unwrap();
    let result = ExecuteHumanTask::new(&FakeLauncher::default(), &FakeObserver).execute(
        task(),
        dir.path().into(),
        dir.path().join("r"),
        &AtomicBool::new(false),
    );
    assert!(result.validate().is_ok());
    assert!(result.final_shell_cwd.is_some());
    assert!(result.shell_log_range.is_some());
    assert!(result.observation.as_ref().unwrap().cwd_exists);

    let bare_done = aibe_protocol::HumanTaskResult {
        status: HandoffExecutionOutcome::Done,
        task: task(),
        human_shell_exit_code: None,
        final_shell_cwd: None,
        shell_log_range: None,
        observation: None,
        error: None,
    };
    assert!(bare_done.validate().is_err());
}

#[test]
fn collab_rerun_restores_execution_mode_without_unknown_human_task() {
    use ai::domain::{
        collaborative_handoff_for_rerun, execution_mode_for_rerun, tools_cli_for_rerun,
    };

    let saved_tools = vec!["shell_exec".into(), "human_task".into()];
    let tools_cli = tools_cli_for_rerun(&saved_tools).expect("tools");
    let mode = execution_mode_for_rerun(false, ExecutionMode::Collaborative);
    let handoff = collaborative_handoff_for_rerun(false, false);
    assert!(
        !handoff,
        "ai collab must not enable legacy shell_exec interception on rerun"
    );
    let plan = plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        Some(tools_cli.as_str()),
        "/tmp/x".into(),
        false,
        mode,
    )
    .expect("plan");
    let names: Vec<_> = plan
        .resolved_tools
        .allowlist
        .names()
        .iter()
        .map(|n| n.as_str())
        .collect();
    assert!(names.contains(&"shell_exec"));
    assert!(names.contains(&"human_task"));
}

#[test]
fn human_task_result_requires_no_manual_summary() {
    let dir = tempfile::tempdir().unwrap();
    let result = ExecuteHumanTask::new(&FakeLauncher::default(), &FakeObserver).execute(
        task(),
        dir.path().into(),
        dir.path().join("r"),
        &AtomicBool::new(false),
    );
    assert!(result
        .observation
        .unwrap()
        .human_task_evidence
        .unwrap()
        .commands
        .is_empty());
}

#[test]
fn human_task_done_does_not_mean_verified() {
    let dir = tempfile::tempdir().unwrap();
    let result = ExecuteHumanTask::new(&FakeLauncher::default(), &FakeObserver).execute(
        task(),
        dir.path().into(),
        dir.path().join("r"),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Done);
    assert_eq!(result.human_shell_exit_code, Some(7));
}

#[test]
fn human_task_is_independent_from_shell_exec() {
    let plan = plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        Some("none"),
        "/tmp/x".into(),
        false,
        ExecutionMode::Collaborative,
    )
    .unwrap();
    assert_eq!(
        plan.resolved_tools.allowlist.names()[0].as_str(),
        "human_task"
    );
}

#[test]
fn collab_instruction_is_mode_scoped_and_not_in_cli() {
    assert!(append_collaborative_instruction(None, ExecutionMode::Normal).is_none());
    let text =
        append_collaborative_instruction(Some("existing".into()), ExecutionMode::Collaborative)
            .unwrap();
    assert!(text.contains("human_task") && text.starts_with("existing"));
}

#[test]
fn collab_human_task_vertical_fragments_use_collaborative_plan_and_fake_executor() {
    let plan = plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        Some("none"),
        "/tmp/x".into(),
        false,
        ExecutionMode::Collaborative,
    )
    .unwrap();
    assert_eq!(
        plan.resolved_tools.allowlist.names()[0].as_str(),
        "human_task"
    );
    let dir = tempfile::tempdir().unwrap();
    let result = ExecuteHumanTask::new(&FakeLauncher::default(), &FakeObserver).execute(
        task(),
        dir.path().into(),
        dir.path().join("r"),
        &AtomicBool::new(false),
    );
    assert_eq!(result.status, HandoffExecutionOutcome::Done);
    assert_eq!(result.task.objective, "inspect workspace");
}

#[test]
fn human_task_result_errors_are_structured() {
    let invalid = aibe_protocol::HumanTaskResult {
        status: HandoffExecutionOutcome::Blocked,
        task: task(),
        human_shell_exit_code: None,
        final_shell_cwd: None,
        shell_log_range: None,
        observation: None,
        error: None,
    };
    assert!(invalid.validate().is_err());
    let valid = aibe_protocol::HumanTaskResult {
        error: Some(HumanHandoffFailure {
            code: "human_task_launch_failed".into(),
            message: "blocked".into(),
        }),
        ..invalid
    };
    assert!(valid.validate().is_ok());
}

#[test]
fn collab_human_task_preserves_prior_stage_regressions() {
    let exec = plan_ask_launch_for_mode(
        &ConfigToolsTokens::default(),
        Some("@exec"),
        "/tmp/x".into(),
        false,
        ExecutionMode::Collaborative,
    )
    .unwrap();
    let names: Vec<_> = exec
        .resolved_tools
        .allowlist
        .names()
        .iter()
        .map(|n| n.as_str())
        .collect();
    assert_eq!(names, ["shell_exec", "human_task"]);
    assert_eq!(
        HandoffExecutionOutcome::HumanControlReturned,
        HandoffExecutionOutcome::HumanControlReturned
    );
}

#[test]
fn collab_docs_use_official_entrypoint() {
    let help = AiCli::command().render_long_help().to_string();
    assert!(help.contains("collab"));
    let manual = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/manual/0062_collab-mode-human-task-tool.md"
    ))
    .unwrap();
    assert!(
        manual.contains("ai collab")
            && manual.contains("--collaborative")
            && manual.contains("@exec")
    );
}
