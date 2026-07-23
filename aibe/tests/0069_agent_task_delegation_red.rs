use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aibe::adapters::outbound::agent_task::{DefaultAgentTaskWorkerRegistry, MockWorker};
use aibe::adapters::outbound::tools::DefaultToolRegistry;
use aibe::adapters::outbound::{ScriptedMockLlm, TomlConfig};
use aibe::application::agent_task::{AgentTaskService, AgentTaskServiceError};
use aibe::application::agent_task_pack::{ActiveAgentTaskPack, AgentTaskPack, BasicAgentTaskPack};
use aibe::application::agent_task_tool::AgentTaskTool;
use aibe::application::task_completion::evidence_from_tools;
use aibe::application::tool_defs::definitions_for;
use aibe::application::tool_round::{RoundOutcome, ToolRoundExecutor};
use aibe::domain::{
    classify_task_completion_eligibility, AgentTaskCriterion, AgentTaskEvidenceSource,
    AgentTaskRequest, AgentTaskResult, AgentTaskStatus, ChatMessage, ClientCwd, DelegationDepth,
    EvidenceSource, ExecutedToolCall, LlmStepResult, TaskCompletionEligibility, TaskContract,
    TaskKind, ToolCall, ToolName, WorkerId, AGENT_TASK, HUMAN_TASK,
};
use aibe::ports::outbound::{
    AgentTaskApprovalGate, AgentTaskApprovalOutcome, AgentTaskApprovalPrompt,
    AgentTaskWorkerConfig, ConfigLoader, NoopLlmCallTracer, ToolExecutionContext, ToolExecutor,
    ToolRegistry, ToolsConfig, WorkerExecutionOutcome, WorkerExecutionOutput,
};
use async_trait::async_trait;
use serde_json::json;
use tempfile::TempDir;

struct ApprovalGate {
    outcome: AgentTaskApprovalOutcome,
    calls: AtomicUsize,
}

impl ApprovalGate {
    fn explicit() -> Arc<Self> {
        Arc::new(Self {
            outcome: AgentTaskApprovalOutcome::Approved {
                origin: "explicit_ui".into(),
            },
            calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl AgentTaskApprovalGate for ApprovalGate {
    async fn request_agent_task_approval(
        &self,
        _tool_call_id: &str,
        prompt: AgentTaskApprovalPrompt,
    ) -> AgentTaskApprovalOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(prompt.trust_boundary_warning.contains("not an OS sandbox"));
        self.outcome.clone()
    }
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/0069_agent_task_worker.sh")
}

fn worker_config(id: &str, mode: &str, timeout_secs: u64) -> AgentTaskWorkerConfig {
    AgentTaskWorkerConfig {
        id: id.into(),
        executable: fixture(),
        args: vec![mode.into()],
        timeout_secs,
        permission_profile: "test-bounded".into(),
        env_allowlist: Vec::new(),
    }
}

fn valid_request(worker: &str) -> AgentTaskRequest {
    AgentTaskRequest {
        worker: WorkerId::parse(worker).expect("worker id"),
        objective: "write deterministic fixture output".into(),
        instructions: vec!["run once".into()],
        completion_criteria: vec![AgentTaskCriterion {
            id: "c1".into(),
            description: "fixture output exists".into(),
        }],
        cwd: None,
        timeout_secs: Some(2),
    }
}

fn context(root: &Path, approval: Arc<dyn AgentTaskApprovalGate>) -> ToolExecutionContext {
    ToolExecutionContext::new(
        ClientCwd::new(root.to_path_buf()).expect("temporary directory is absolute"),
    )
    .with_agent_task_approval_gate(approval)
}

fn service_from_configs(configs: &[AgentTaskWorkerConfig], root: &Path) -> Arc<AgentTaskService> {
    let registry = DefaultAgentTaskWorkerRegistry::from_configs(configs).expect("registry");
    Arc::new(AgentTaskService::new(
        Arc::new(registry),
        true,
        vec![root.to_path_buf()],
        4096,
        1800,
    ))
}

fn mock_output(outcome: WorkerExecutionOutcome) -> WorkerExecutionOutput {
    WorkerExecutionOutput {
        outcome,
        summary: "mock result".into(),
        reported_complete: true,
        blockers: Vec::new(),
        stdout: "bounded".into(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        exit_code: Some(0),
        changed_paths: Vec::new(),
        observation_incomplete: false,
    }
}

#[tokio::test]
async fn agent_task_vertical_e2e() {
    let temp = TempDir::new().expect("tempdir");
    let config_path = temp.path().join("config.toml");
    let config_text = format!(
        "[agent_task]\nenabled = true\n[[agent_task.workers]]\nid = \"fixture\"\nexecutable = {:?}\nargs = [\"success\"]\ntimeout_secs = 2\npermission_profile = \"test-bounded\"\n",
        fixture().to_string_lossy()
    );
    std::fs::write(&config_path, config_text).expect("config");
    let parsed = TomlConfig::from_path(config_path)
        .load()
        .expect("production parser");
    assert!(parsed.agent_task.enabled);
    let service = service_from_configs(&parsed.agent_task.workers, temp.path());
    let tool = Arc::new(AgentTaskTool::new(service)) as Arc<dyn ToolExecutor>;
    let registry: Arc<dyn ToolRegistry> =
        Arc::new(DefaultToolRegistry::from_executors([tool]).expect("production tool registry"));
    let call = ToolCall {
        id: "delegate-1".into(),
        name: AGENT_TASK.into(),
        arguments: serde_json::to_value(valid_request("fixture")).expect("request json"),
        provider_extras: None,
    };
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::with_tool_calls("", vec![call]),
        LlmStepResult::text_only("parent resumed"),
    ]));
    let executor = ToolRoundExecutor::new(
        llm.clone(),
        registry,
        ToolsConfig::default(),
        Arc::new(NoopLlmCallTracer),
    );
    let approval = ApprovalGate::explicit();
    let ctx = context(temp.path(), approval.clone());
    let first = executor
        .run_one_round(
            &[ChatMessage::user("delegate")],
            &[ToolName::agent_task()],
            &[],
            &ctx,
            &[],
            None,
            None,
        )
        .await
        .expect("first round");
    let (conversation, executed) = match first {
        RoundOutcome::Continue {
            conversation,
            executed,
        } => (conversation, executed),
        other => panic!("expected Continue, got {other:?}"),
    };
    let result: AgentTaskResult =
        serde_json::from_str(executed[0].output.as_deref().expect("tool output"))
            .expect("structured result");
    assert_eq!(result.status, AgentTaskStatus::Completed, "{result:#?}");
    assert!(!result.verified);
    assert!(result.evidence.iter().all(|item| !item.verified));
    assert_eq!(result.approval_origin, "explicit_ui");
    assert!(temp.path().join("agent-task-output.txt").is_file());
    let second = executor
        .run_one_round(
            &conversation,
            &[ToolName::agent_task()],
            &[],
            &ctx,
            &executed,
            None,
            None,
        )
        .await
        .expect("parent resumes");
    assert!(matches!(second, RoundOutcome::Completed { .. }));
    assert_eq!(llm.recorded_calls().len(), 2);
    assert_eq!(approval.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn agent_task_request_is_strictly_validated() {
    let definition = definitions_for(&[ToolName::agent_task()])
        .into_iter()
        .next()
        .expect("agent_task definition");
    let criterion =
        &definition.parameters["properties"]["completion_criteria"]["items"]["properties"];
    assert_eq!(criterion["id"]["minLength"], 1);
    assert_eq!(criterion["id"]["maxLength"], 64);
    assert_eq!(criterion["description"]["minLength"], 1);
    assert_eq!(criterion["description"]["maxLength"], 2048);
    let unknown = json!({
        "worker":"fixture", "objective":"x", "instructions":["x"],
        "completion_criteria":[{"id":"c1","description":"x"}], "executable":"sh"
    });
    assert!(serde_json::from_value::<AgentTaskRequest>(unknown).is_err());
    for forbidden in [
        "argv",
        "env",
        "permission_profile",
        "approval",
        "delegation_depth",
    ] {
        let mut value = serde_json::to_value(valid_request("fixture")).expect("json");
        value[forbidden] = json!(true);
        assert!(serde_json::from_value::<AgentTaskRequest>(value).is_err());
    }
    let mut empty = valid_request("fixture");
    empty.objective.clear();
    assert!(empty.validate(10, 10).is_err());
    let mut duplicate = valid_request("fixture");
    duplicate
        .completion_criteria
        .push(duplicate.completion_criteria[0].clone());
    assert!(duplicate.validate(10, 10).is_err());
    let mut timeout = valid_request("fixture");
    timeout.timeout_secs = Some(11);
    assert!(timeout.validate(10, 10).is_err());
    assert!(WorkerId::parse("Uppercase").is_err());
}

#[test]
fn agent_task_registry_and_disabled_pack_fail_closed() {
    let basic = BasicAgentTaskPack::default();
    assert!(!basic.publishes_tool());
    assert!(basic.registry().is_empty());
    let duplicate = vec![
        worker_config("fixture", "success", 2),
        worker_config("fixture", "success", 2),
    ];
    assert!(DefaultAgentTaskWorkerRegistry::from_configs(&duplicate).is_err());
    let configured =
        DefaultAgentTaskWorkerRegistry::from_configs(&[worker_config("fixture", "success", 2)])
            .expect("configured registry");
    let active = ActiveAgentTaskPack::new(Arc::new(configured));
    assert!(active.publishes_tool());
    assert!(active
        .registry()
        .get(&WorkerId::parse("unknown").expect("id"))
        .is_none());
    let defs = definitions_for(&[ToolName::human_task()]);
    assert!(defs.iter().all(|definition| definition.name != AGENT_TASK));
}

#[tokio::test]
async fn agent_task_core_is_product_agnostic_and_mockable() {
    let temp = TempDir::new().expect("tempdir");
    for outcome in [
        WorkerExecutionOutcome::Completed,
        WorkerExecutionOutcome::Failed,
        WorkerExecutionOutcome::TimedOut,
        WorkerExecutionOutcome::InvalidOutput,
    ] {
        let mock = Arc::new(MockWorker::new(Ok(mock_output(outcome.clone()))));
        let registry = DefaultAgentTaskWorkerRegistry::from_workers(vec![(
            WorkerId::parse("mock").expect("id"),
            mock.clone(),
            2,
            "mock-profile".into(),
        )])
        .expect("registry");
        let service = AgentTaskService::new(
            Arc::new(registry),
            true,
            vec![temp.path().to_path_buf()],
            4096,
            1800,
        );
        let approval = ApprovalGate::explicit();
        let result = service
            .execute("c1", valid_request("mock"), &context(temp.path(), approval))
            .await
            .expect("normalized result");
        assert!(!result.verified);
        assert_eq!(mock.calls().len(), 1);
    }
}

#[tokio::test]
async fn agent_task_runs_in_validated_cwd_with_timeout() {
    let temp = TempDir::new().expect("tempdir");
    let outside = TempDir::new().expect("outside tempdir");
    let child = temp.path().join("child");
    std::fs::create_dir(&child).expect("child");
    std::os::unix::fs::symlink(outside.path(), temp.path().join("escape-link"))
        .expect("symlink fixture");
    let service = service_from_configs(
        &[
            worker_config("ok", "success", 2),
            worker_config("slow", "timeout", 1),
        ],
        temp.path(),
    );
    let approval = ApprovalGate::explicit();
    let mut relative = valid_request("ok");
    relative.cwd = Some("child".into());
    assert_eq!(
        service
            .execute(
                "relative",
                relative,
                &context(temp.path(), approval.clone())
            )
            .await
            .expect("relative cwd")
            .status,
        AgentTaskStatus::Completed
    );
    let mut absolute = valid_request("ok");
    absolute.cwd = Some(child.to_string_lossy().into_owned());
    assert_eq!(
        service
            .execute(
                "absolute",
                absolute,
                &context(temp.path(), approval.clone())
            )
            .await
            .expect("root-contained absolute cwd")
            .status,
        AgentTaskStatus::Completed
    );
    for cwd in ["../", "missing", "agent-task-file", "escape-link"] {
        if cwd == "agent-task-file" {
            std::fs::write(temp.path().join(cwd), "file").expect("file");
        }
        let mut request = valid_request("ok");
        request.cwd = Some(cwd.into());
        assert_eq!(
            service
                .execute("bad-cwd", request, &context(temp.path(), approval.clone()))
                .await
                .expect_err("cwd rejected"),
            AgentTaskServiceError::InvalidCwd
        );
    }
    let mut timeout = valid_request("slow");
    timeout.timeout_secs = Some(1);
    let result = service
        .execute("timeout", timeout, &context(temp.path(), approval))
        .await
        .expect("timeout result");
    assert_eq!(result.status, AgentTaskStatus::TimedOut);
    assert!(result.timed_out);
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!temp.path().join("agent-task-timeout-sentinel.txt").exists());
    let child_pid: i32 = std::fs::read_to_string(temp.path().join("agent-task-child.pid"))
        .expect("child pid")
        .trim()
        .parse()
        .expect("numeric pid");
    let alive = unsafe { libc::kill(child_pid, 0) } == 0;
    assert!(!alive, "timeout child process must be reaped");
}

#[tokio::test]
async fn agent_task_result_normalizes_worker_outcomes() {
    let temp = TempDir::new().expect("tempdir");
    let missing = temp.path().join("does-not-exist");
    let configs = vec![
        worker_config("nonzero", "nonzero", 2),
        worker_config("malformed", "malformed", 2),
        worker_config("large", "large", 2),
        AgentTaskWorkerConfig {
            id: "launch".into(),
            executable: missing,
            args: Vec::new(),
            timeout_secs: 2,
            permission_profile: "test".into(),
            env_allowlist: Vec::new(),
        },
    ];
    let service = service_from_configs(&configs, temp.path());
    let approval = ApprovalGate::explicit();
    for (worker, status) in [
        ("nonzero", AgentTaskStatus::Failed),
        ("malformed", AgentTaskStatus::InvalidOutput),
        ("launch", AgentTaskStatus::LaunchFailed),
    ] {
        let result = service
            .execute(
                worker,
                valid_request(worker),
                &context(temp.path(), approval.clone()),
            )
            .await
            .expect("normalized outcome");
        assert_eq!(result.status, status);
        assert!(!result.reported_complete);
        assert!(!result.verified);
    }
    let large = service
        .execute(
            "large",
            valid_request("large"),
            &context(temp.path(), approval),
        )
        .await
        .expect("large output normalized");
    assert_eq!(large.status, AgentTaskStatus::InvalidOutput);
    assert!(large.stdout_truncated);
    assert!(large.stdout.len() <= 4096);
}

#[tokio::test]
async fn agent_task_evidence_is_bounded_and_unverified() {
    let temp = TempDir::new().expect("tempdir");
    let service = service_from_configs(&[worker_config("fixture", "success", 2)], temp.path());
    let result = service
        .execute(
            "evidence",
            valid_request("fixture"),
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("result");
    assert!(result
        .changed_paths
        .contains(&PathBuf::from("agent-task-output.txt")));
    assert!(result.changed_paths.len() <= 256);
    assert!(!result.verified);
    assert!(result.evidence.iter().all(|item| !item.verified));
    assert!(result.evidence.iter().any(|item| {
        item.source == AgentTaskEvidenceSource::WorkspaceObserver
            && item.summary.contains("agent-task-output.txt")
    }));
    assert!(result
        .evidence
        .iter()
        .all(|item| item.summary.len() <= 1024));
}

#[tokio::test]
async fn agent_task_recursion_is_rejected() {
    let temp = TempDir::new().expect("tempdir");
    let mock = Arc::new(MockWorker::new(Ok(mock_output(
        WorkerExecutionOutcome::Completed,
    ))));
    let registry = DefaultAgentTaskWorkerRegistry::from_workers(vec![(
        WorkerId::parse("mock").expect("id"),
        mock.clone(),
        2,
        "mock".into(),
    )])
    .expect("registry");
    let service = AgentTaskService::new(
        Arc::new(registry),
        true,
        vec![temp.path().to_path_buf()],
        4096,
        1800,
    );
    let approval = ApprovalGate::explicit();
    let delegated =
        context(temp.path(), approval.clone()).with_delegation_depth(DelegationDepth::delegated());
    assert!(!service.published_for(DelegationDepth::delegated()));
    assert_eq!(
        service
            .execute("forged", valid_request("mock"), &delegated)
            .await
            .expect_err("recursive call rejected"),
        AgentTaskServiceError::RecursiveDelegation
    );
    assert!(mock.calls().is_empty());
    assert_eq!(approval.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn agent_task_approval_cannot_be_bypassed() {
    let temp = TempDir::new().expect("tempdir");
    let mock = Arc::new(MockWorker::new(Ok(mock_output(
        WorkerExecutionOutcome::Completed,
    ))));
    let registry = DefaultAgentTaskWorkerRegistry::from_workers(vec![(
        WorkerId::parse("mock").expect("id"),
        mock.clone(),
        2,
        "mock".into(),
    )])
    .expect("registry");
    let service = Arc::new(AgentTaskService::new(
        Arc::new(registry),
        true,
        vec![temp.path().to_path_buf()],
        4096,
        1800,
    ));
    for outcome in [
        AgentTaskApprovalOutcome::Denied {
            origin: "ui_no".into(),
        },
        AgentTaskApprovalOutcome::Unavailable,
        AgentTaskApprovalOutcome::Timeout,
        AgentTaskApprovalOutcome::Cancelled,
        AgentTaskApprovalOutcome::Approved {
            origin: "shell_allowlist".into(),
        },
    ] {
        let gate = Arc::new(ApprovalGate {
            outcome,
            calls: AtomicUsize::new(0),
        });
        assert!(service
            .execute("denied", valid_request("mock"), &context(temp.path(), gate))
            .await
            .is_err());
    }
    assert!(mock.calls().is_empty());
    let approved = service
        .execute(
            "approved",
            valid_request("mock"),
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("explicit approval");
    assert_eq!(approved.approval_origin, "explicit_ui");
    assert_eq!(mock.calls().len(), 1);

    let tool = AgentTaskTool::new(Arc::clone(&service));
    let (record, result) = tool
        .execute(
            "audit-1",
            &json!({
                "worker": "mock",
                "objective": "write deterministic fixture output",
                "instructions": ["run once"],
                "completion_criteria": [{"id":"c1","description":"fixture output exists"}],
                "timeout_secs": 2
            }),
            1000,
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await;
    assert!(!result.is_error);
    let source = record.approval_source.as_deref().expect("approval_source");
    assert!(
        source.starts_with("agent_task_approval=ask;"),
        "unexpected approval_source={source}"
    );
    assert!(source.contains("worker=mock"));
    assert!(source.contains(&format!("cwd={}", temp.path().display())));
    assert!(source.contains("timeout=2"));
    assert!(source.contains("origin=explicit_ui"));
    assert!(source.contains("approval=approved"));
    assert!(!source.contains("client_tools_allowlist"));
    assert_eq!(
        record.risk_class,
        Some(aibe_protocol::ToolRiskClass::WriteLike)
    );
    assert_eq!(record.decision.as_deref(), Some("executed"));
}

#[test]
fn agent_task_integrates_with_task_completion_as_unverified() {
    assert!(matches!(
        classify_task_completion_eligibility(true, &[AGENT_TASK]),
        TaskCompletionEligibility::Active { .. }
    ));
    let result = AgentTaskResult::unverified(
        AgentTaskStatus::Completed,
        "worker claims done",
        true,
        Vec::new(),
        String::new(),
        String::new(),
        false,
        false,
        Some(0),
        false,
        vec![PathBuf::from("changed.txt")],
        false,
        Vec::new(),
        "explicit_ui",
        "fixture",
        "/tmp/task",
        2,
    );
    let contract = TaskContract {
        goal: "delegate and verify a change".into(),
        task_kind: TaskKind::Execution,
        criteria: vec![aibe::domain::CompletionCriterion {
            id: "c1".into(),
            description: "delegated work is independently verified".into(),
            deliverable_is_plan: false,
            observes_targets: Vec::new(),
            applicability: None,
        }],
        constraints: Vec::new(),
        deliverables: vec!["changed file".into()],
        verification: vec!["read changed file".into()],
        verification_tools: vec!["read_file".into()],
        delegated_verification: None,
    };
    let prior = ExecutedToolCall::ok(
        "obs-1".into(),
        ToolName::read_file(),
        json!({"path":"changed.txt"}),
        "old contents".to_string(),
    )
    .with_audit(
        aibe_protocol::ToolRiskClass::ReadOnly,
        aibe_protocol::ToolApprovalState::NotRequired,
        false,
    );
    let mut ledger = evidence_from_tools(&contract, &[prior]);
    assert_eq!(ledger.len(), 1);
    ledger[0].verified = true;
    ledger[0].source = EvidenceSource::Observation;

    let verification = ExecutedToolCall::ok(
        "ver-1".into(),
        ToolName::read_file(),
        json!({"path":"changed.txt"}),
        "verified once".to_string(),
    )
    .with_audit(
        aibe_protocol::ToolRiskClass::ReadOnly,
        aibe_protocol::ToolApprovalState::NotRequired,
        false,
    );
    ledger = aibe::application::task_completion::append_evidence_from_tools(
        &contract,
        &ledger,
        &[verification],
    );
    if let Some(record) = ledger.last_mut() {
        record.verified = true;
        record.source = EvidenceSource::Verification;
    }

    let call = ExecutedToolCall::ok(
        "agent-1".into(),
        ToolName::agent_task(),
        json!({"worker":"fixture"}),
        serde_json::to_string(&result).expect("result json"),
    )
    .with_agent_task_audit(
        aibe_protocol::AgentTaskApprovalAudit::Approved,
        "fixture",
        "/tmp/task",
        2,
        "explicit_ui",
    );
    let ledger =
        aibe::application::task_completion::append_evidence_from_tools(&contract, &ledger, &[call]);
    assert!(ledger
        .iter()
        .any(|r| r.source == EvidenceSource::AgentTask && !r.verified));
    assert!(ledger
        .iter()
        .filter(|r| matches!(
            r.source,
            EvidenceSource::Observation | EvidenceSource::Verification
        ))
        .all(|r| r.stale && !r.verified));
}

#[test]
fn agent_task_preserves_human_task_behavior() {
    assert_ne!(AGENT_TASK, HUMAN_TASK);
    let definitions = definitions_for(&[ToolName::human_task(), ToolName::agent_task()]);
    let human = definitions
        .iter()
        .find(|definition| definition.name == HUMAN_TASK)
        .expect("human_task remains published independently");
    assert_eq!(human.parameters["required"], json!(["objective"]));
    assert!(human.parameters["properties"]["suggested_commands"].is_object());
    assert!(human.description.contains("interactive Human Shell"));
}

#[test]
fn agent_task_rejects_relative_executable() {
    use aibe::ports::outbound::{validate_agent_task_config, AgentTaskConfig};
    let mut config = AgentTaskConfig {
        enabled: true,
        workers: vec![AgentTaskWorkerConfig {
            id: "relative".into(),
            executable: PathBuf::from("./worker"),
            args: Vec::new(),
            timeout_secs: 2,
            permission_profile: "test".into(),
            env_allowlist: Vec::new(),
        }],
    };
    let err = validate_agent_task_config(&mut config).expect_err("relative executable rejected");
    assert!(
        err.to_string().contains("absolute path"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn agent_task_timeout_covers_orphan_stdout_drain() {
    let temp = TempDir::new().expect("tempdir");
    let service = service_from_configs(&[worker_config("orphan", "orphan_pipe", 1)], temp.path());
    let mut request = valid_request("orphan");
    request.timeout_secs = Some(1);
    let started = std::time::Instant::now();
    let result = service
        .execute(
            "orphan",
            request,
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("timeout result");
    assert_eq!(result.status, AgentTaskStatus::TimedOut);
    assert!(result.timed_out);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "drain must not hang past timeout"
    );
}

#[tokio::test]
async fn agent_task_pre_approval_errors_are_not_audited_as_approved() {
    let temp = TempDir::new().expect("tempdir");
    let service = service_from_configs(&[worker_config("fixture", "success", 2)], temp.path());
    let tool = AgentTaskTool::new(service);
    let (record, result) = tool
        .execute(
            "unknown-worker",
            &json!({
                "worker": "missing",
                "objective": "write deterministic fixture output",
                "instructions": ["run once"],
                "completion_criteria": [{"id":"c1","description":"fixture output exists"}],
                "timeout_secs": 2
            }),
            1000,
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await;
    assert!(result.is_error);
    assert_eq!(
        record.approval_state,
        Some(aibe_protocol::ToolApprovalState::NotRequired)
    );
    assert_eq!(record.decision.as_deref(), Some("rejected_before_approval"));
    let source = record.approval_source.as_deref().expect("approval_source");
    assert!(source.contains("approval=not_requested"));
    assert!(!source.contains("approval=approved"));
}

#[tokio::test]
async fn agent_task_blocked_is_structured_for_parent() {
    let temp = TempDir::new().expect("tempdir");
    let service = service_from_configs(&[worker_config("blocked", "blocked", 2)], temp.path());
    let result = service
        .execute(
            "blocked",
            valid_request("blocked"),
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("blocked result");
    assert_eq!(result.status, AgentTaskStatus::Blocked);
    assert!(!result.reported_complete);
    assert_eq!(result.blockers, vec!["missing API credential".to_string()]);
    assert!(!result.verified);
}

#[tokio::test]
async fn agent_task_redacts_worker_secrets_before_parent_prompt() {
    let temp = TempDir::new().expect("tempdir");
    let service = service_from_configs(&[worker_config("secret", "secret", 2)], temp.path());
    let result = service
        .execute(
            "secret",
            valid_request("secret"),
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("secret result");
    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert!(!result.summary.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(result.summary.contains("sk-[REDACTED]"));
    assert!(!result.stderr.contains("super-secret-token-value"));
    assert!(result.stderr.contains("TOKEN=[REDACTED]"));
    let serialized = serde_json::to_string(&result).expect("serialize result");
    assert!(
        !serialized.contains("sk-abcdefghijklmnopqrstuvwxyz"),
        "secret must not appear anywhere in serialized result"
    );
    assert!(
        !serialized.contains("super-secret-token-value"),
        "env secret must not appear anywhere in serialized result"
    );
    assert!(result.evidence.iter().all(|item| {
        !item.summary.contains("sk-abcdefghijklmnopqrstuvwxyz")
            && !item.summary.contains("super-secret-token-value")
    }));
}

#[tokio::test]
async fn agent_task_redacts_inherited_env_values_even_without_key_prefix() {
    let _env_lock = ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let secret = "aws-exact-value-credential-9f3a2b1c0d";
    let env_name = "AWS_SECRET_ACCESS_KEY";
    let _guard = EnvVarGuard::set(env_name, secret);
    let temp = TempDir::new().expect("tempdir");
    let mut config = worker_config("envsecret", "env_value_secret", 2);
    config.env_allowlist = vec![env_name.into()];
    let service = service_from_configs(&[config], temp.path());
    let result = service
        .execute(
            "envsecret",
            valid_request("envsecret"),
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("env secret result");
    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert!(
        !result.stderr.contains(secret),
        "bare inherited credential on stderr must be exact-redacted"
    );
    assert!(
        !result.summary.contains(secret),
        "bare inherited credential in summary must be exact-redacted"
    );
    assert!(result.stderr.contains("[REDACTED]"));
    assert!(result.summary.contains("[REDACTED]"));
    let serialized = serde_json::to_string(&result).expect("serialize result");
    assert!(
        !serialized.contains(secret),
        "inherited credential must not appear in serialized parent result"
    );
}

#[tokio::test]
async fn agent_task_redacts_truncated_inherited_env_prefix() {
    let _env_lock = ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let secret = "aws-truncated-boundary-credential-value-xyz";
    let env_name = "AWS_SECRET_ACCESS_KEY";
    let _guard = EnvVarGuard::set(env_name, secret);
    let temp = TempDir::new().expect("tempdir");
    let mut config = worker_config("truncsecret", "truncated_env_secret", 2);
    config.env_allowlist = vec![env_name.into()];
    // 4080 pad + secret; keep 4096 bytes so only a proper prefix of the secret remains.
    let service = Arc::new(AgentTaskService::new(
        Arc::new(DefaultAgentTaskWorkerRegistry::from_configs(&[config]).expect("registry")),
        true,
        vec![temp.path().to_path_buf()],
        4096,
        1800,
    ));
    let result = service
        .execute(
            "truncsecret",
            valid_request("truncsecret"),
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("truncated env secret result");
    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert!(result.stderr_truncated);
    assert!(
        !result.stderr.contains(secret),
        "full secret must not appear after truncation"
    );
    let kept_prefix = &secret[..secret.len().min(16)];
    assert!(
        !result.stderr.contains(kept_prefix),
        "truncated secret prefix must be redacted, got stderr={}",
        result.stderr
    );
    assert!(result.stderr.contains("[REDACTED]"));
}

#[tokio::test]
async fn agent_task_excludes_changed_paths_that_embed_inherited_secrets() {
    let _env_lock = ENV_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let secret = "aws-path-secret-value-9f3a2b1c";
    let env_name = "AWS_SECRET_ACCESS_KEY";
    let _guard = EnvVarGuard::set(env_name, secret);
    let temp = TempDir::new().expect("tempdir");
    let mut config = worker_config("pathsecret", "secret_named_path", 2);
    config.env_allowlist = vec![env_name.into()];
    let service = service_from_configs(&[config], temp.path());
    let result = service
        .execute(
            "pathsecret",
            valid_request("pathsecret"),
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("secret-named path result");
    assert_eq!(result.status, AgentTaskStatus::Completed);
    assert!(
        result.observation_incomplete,
        "dropping secret-bearing paths must mark observation incomplete"
    );
    assert!(
        result
            .changed_paths
            .iter()
            .all(|path| !path.to_string_lossy().contains(secret)),
        "changed_paths must not embed inherited credentials: {:?}",
        result.changed_paths
    );
    let serialized = serde_json::to_string(&result).expect("serialize result");
    assert!(
        !serialized.contains(secret),
        "inherited credential must not appear anywhere in serialized parent result"
    );
    assert!(result
        .evidence
        .iter()
        .all(|item| !item.summary.contains(secret)));
}

/// Serializes process-wide env mutation used by Agent Task secret redaction tests.
static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Restores a process env var on drop so panics cannot leak test credentials.
struct EnvVarGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        // SAFETY: test-only; Drop restores the previous value.
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

#[tokio::test]
async fn agent_task_drains_output_before_stdin_write() {
    // Integration coverage: fixture emits pipe-filling stderr before reading stdin.
    // The definitive large-stdin deadlock regression lives in
    // `subprocess::tests::bounded_run_drains_before_stdin_write_avoids_pipe_deadlock`
    // (request shape caps keep agent_task envelopes below typical pipe capacity).
    let temp = TempDir::new().expect("tempdir");
    let service = service_from_configs(&[worker_config("spam", "startup_spam", 5)], temp.path());
    let mut request = valid_request("spam");
    request.timeout_secs = Some(5);
    let started = std::time::Instant::now();
    let result = service
        .execute(
            "spam",
            request,
            &context(temp.path(), ApprovalGate::explicit()),
        )
        .await
        .expect("startup spam result");
    assert_eq!(
        result.status,
        AgentTaskStatus::Completed,
        "Worker that fills stderr before reading stdin must complete, not TimedOut"
    );
    assert!(!result.timed_out);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "concurrent drain must avoid pipe-deadlock timeout"
    );
}

#[tokio::test]
async fn agent_task_denied_approval_is_audited_as_explicit_opt_in() {
    let temp = TempDir::new().expect("tempdir");
    let service = service_from_configs(&[worker_config("fixture", "success", 2)], temp.path());
    let tool = AgentTaskTool::new(service);
    let gate = Arc::new(ApprovalGate {
        outcome: AgentTaskApprovalOutcome::Denied {
            origin: "ui_no".into(),
        },
        calls: AtomicUsize::new(0),
    });
    let (record, result) = tool
        .execute(
            "denied",
            &json!({
                "worker": "fixture",
                "objective": "write deterministic fixture output",
                "instructions": ["run once"],
                "completion_criteria": [{"id":"c1","description":"fixture output exists"}],
                "timeout_secs": 2
            }),
            1000,
            &context(temp.path(), gate),
        )
        .await;
    assert!(result.is_error);
    assert_eq!(
        record.approval_state,
        Some(aibe_protocol::ToolApprovalState::ExplicitClientOptIn)
    );
    assert_eq!(record.decision.as_deref(), Some("rejected_by_user"));
    let source = record.approval_source.as_deref().expect("approval_source");
    assert!(source.contains("approval=denied"));
}
