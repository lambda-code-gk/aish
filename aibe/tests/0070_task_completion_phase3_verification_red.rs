//! Acceptance tests for spec 0070 Task Completion Phase 3.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aibe::adapters::outbound::agent_task::DefaultAgentTaskWorkerRegistry;
use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry_with_extras;
use aibe::adapters::outbound::{ConversationStore, ScriptedMockLlm, StaticCapabilityPolicy};
use aibe::application::agent_task::AgentTaskService;
use aibe::application::agent_task_tool::AgentTaskTool;
use aibe::application::completion_envelope::ContractGate;
use aibe::application::task_completion::{
    append_evidence_from_tools, attach_gap_report, build_report, evidence_from_tools,
    validate_delegated_cycle_calls,
};
use aibe::application::{basic_pack_arc, build_file_change_executor, RequestService};
use aibe::domain::*;
use aibe::ports::outbound::*;
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, CompletionOutcome as WireOutcome,
    CompletionReport, ErrorCode, ExecutionMode, HandoffExecutionOutcome, HumanTaskRequest,
    HumanTaskResult, PostHandoffObservation, ProtocolMessage, RequestContext,
    ShellExecApprovalOutcome, ShellLogRange, ToolApprovalOrigin,
};
use async_trait::async_trait;
use serde_json::json;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/0069_agent_task_worker.sh")
}

fn contract(cwd: &Path) -> TaskContract {
    TaskContract {
        goal: "delegate, repair, and independently verify artifact".into(),
        task_kind: TaskKind::Execution,
        criteria: vec![CompletionCriterion {
            id: "c1".into(),
            description: "0070-artifact.txt contains verified".into(),
            deliverable_is_plan: false,
            observes_targets: vec![],
            applicability: None,
        }],
        constraints: vec!["same worker and cwd".into()],
        deliverables: vec!["0070-artifact.txt".into()],
        verification: vec!["run fixed check then read artifact".into()],
        verification_tools: vec!["read_file".into(), "git_diff".into()],
        delegated_verification: Some(DelegatedVerificationPlan {
            items: vec![
                DelegatedVerificationPlanItem {
                    id: "v-command".into(),
                    criterion_ids: vec!["c1".into()],
                    action: DelegatedVerificationAction::Command {
                        command: "test".into(),
                        args: vec!["-f".into(), "0070-artifact.txt".into()],
                        cwd: cwd.display().to_string(),
                    },
                    expected_success: "exit status 0".into(),
                },
                DelegatedVerificationPlanItem {
                    id: "v-read".into(),
                    criterion_ids: vec!["c1".into()],
                    action: DelegatedVerificationAction::Observation {
                        tool: "read_file".into(),
                        target: "0070-artifact.txt".into(),
                    },
                    expected_success: "content is verified".into(),
                },
                DelegatedVerificationPlanItem {
                    id: "v-diff".into(),
                    criterion_ids: vec!["c1".into()],
                    action: DelegatedVerificationAction::Observation {
                        tool: "git_diff".into(),
                        target: "0070-artifact.txt".into(),
                    },
                    expected_success: "tracked artifact diff is observed".into(),
                },
            ],
        }),
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git fixture command");
    assert!(status.success(), "git {:?}", args);
}

fn initial_request() -> AgentTaskRequest {
    AgentTaskRequest {
        worker: WorkerId::parse("fixture").expect("worker"),
        objective: "write the artifact".into(),
        instructions: vec!["produce the requested artifact".into()],
        completion_criteria: vec![AgentTaskCriterion {
            id: "c1".into(),
            description: "0070-artifact.txt contains verified".into(),
        }],
        cwd: None,
        timeout_secs: Some(3),
    }
}

fn follow_up_request(contract: &TaskContract) -> AgentTaskRequest {
    let evaluation = evaluation(CriterionStatus::Unsatisfied, &[], Some("repair artifact"));
    let gap = build_gap(contract, &evaluation).expect("gap");
    gap_follow_up_request(contract, &initial_request(), &gap).expect("follow-up")
}

fn evaluation(
    status: CriterionStatus,
    evidence_ids: &[&str],
    next: Option<&str>,
) -> CompletionEvaluation {
    CompletionEvaluation {
        criteria: vec![CriterionEvaluation {
            criterion_id: "c1".into(),
            status,
            evidence_ids: evidence_ids.iter().map(|id| (*id).into()).collect(),
            required_evidence: matches!(
                status,
                CriterionStatus::Unsatisfied | CriterionStatus::Unknown
            )
            .then(|| vec!["artifact content did not meet the criterion".into()])
            .unwrap_or_default(),
            applicability_evidence_ids: vec![],
        }],
        next_objective: next.map(str::to_string),
        needs_user: None,
        blocked: None,
        failure: None,
    }
}

fn envelope(
    contract: &TaskContract,
    evaluation: Option<CompletionEvaluation>,
    body: &str,
) -> String {
    json!({
        "aish_task_completion": {"contract": contract, "evaluation": evaluation},
        "deliverable": body
    })
    .to_string()
}

struct Approval {
    calls: AtomicUsize,
}

struct SuspendedHumanTaskGate;

#[async_trait]
impl HumanTaskGate for SuspendedHumanTaskGate {
    async fn execute_human_task(&self, _: &str, task: HumanTaskRequest) -> Option<HumanTaskResult> {
        Some(HumanTaskResult {
            status: HandoffExecutionOutcome::Suspended,
            task,
            verified: false,
            human_shell_exit_code: Some(0),
            final_shell_cwd: Some("/tmp".into()),
            shell_log_range: Some(ShellLogRange {
                start: 1,
                end: Some(2),
            }),
            observation: Some(PostHandoffObservation {
                cwd_exists: true,
                cwd: "/tmp".into(),
                git_head: None,
                git_branch: None,
                git_status: None,
                shell_log_tail: None,
                shell_log_truncated: None,
                observation_errors: vec![],
                human_task_evidence: None,
            }),
            error: None,
            task_id: Some("ht-20260724-abcdef".into()),
            suspend_reason: Some("manual verification required".into()),
        })
    }
}

#[async_trait]
impl ToolApprovalGate for Approval {
    async fn request_tool_approval(
        &self,
        _: &str,
        prompt: ToolApprovalPromptRequest,
    ) -> ToolApprovalGateOutcome {
        assert_eq!(prompt.tool_name, AGENT_TASK);
        self.calls.fetch_add(1, Ordering::SeqCst);
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes)
    }
}

#[tokio::test]
async fn delegated_verification_vertical_e2e() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    git(dir.path(), &["init", "-q"]);
    std::fs::write(dir.path().join("0070-artifact.txt"), "baseline\n").expect("baseline");
    git(dir.path(), &["add", "0070-artifact.txt"]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=aish-test",
            "-c",
            "user.email=aish-test@example.invalid",
            "commit",
            "-q",
            "-m",
            "baseline",
        ],
    );
    let contract = contract(dir.path());
    let initial = initial_request();
    let follow_up = follow_up_request(&contract);
    let contract_only = || envelope(&contract, None, "");
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "worker-1".into(),
                name: AGENT_TASK.into(),
                arguments: serde_json::to_value(&initial).expect("request"),
                provider_extras: None,
            }],
        ),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "check-1".into(),
                name: SHELL_EXEC.into(),
                arguments: json!({"command":"test","args":["-f","0070-artifact.txt"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "read-1".into(),
                name: READ_FILE.into(),
                arguments: json!({"path":"0070-artifact.txt"}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "diff-1".into(),
                name: GIT_DIFF.into(),
                arguments: json!({"path":"0070-artifact.txt"}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only(envelope(
            &contract,
            Some(evaluation(
                CriterionStatus::Unsatisfied,
                &[],
                Some("repair artifact"),
            )),
            "worker claim rejected",
        )),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "worker-2".into(),
                name: AGENT_TASK.into(),
                arguments: serde_json::to_value(&follow_up).expect("request"),
                provider_extras: None,
            }],
        ),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "check-2".into(),
                name: SHELL_EXEC.into(),
                arguments: json!({"command":"test","args":["-f","0070-artifact.txt"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "read-2".into(),
                name: READ_FILE.into(),
                arguments: json!({"path":"0070-artifact.txt"}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "diff-2".into(),
                name: GIT_DIFF.into(),
                arguments: json!({"path":"0070-artifact.txt"}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only(envelope(
            &contract,
            Some(evaluation(
                CriterionStatus::Satisfied,
                &["e6", "e7", "e8"],
                None,
            )),
            "independently verified",
        )),
    ]));
    let registry = DefaultAgentTaskWorkerRegistry::from_configs(&[AgentTaskWorkerConfig {
        id: "fixture".into(),
        executable: fixture(),
        args: vec!["gap_repair".into()],
        timeout_secs: 3,
        permission_profile: "test-bounded".into(),
        env_allowlist: vec![],
    }])
    .expect("registry");
    let agent_service = Arc::new(AgentTaskService::new(
        Arc::new(registry),
        true,
        vec![dir.path().to_path_buf()],
        8192,
        1800,
    ));
    let tools = ToolsConfig {
        shell_exec: ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["test".into()],
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        },
        read_file: ReadFileConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
        },
        file_write: FileWriteConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
            ..Default::default()
        },
        ..Default::default()
    };
    let registry = build_registry_with_extras(
        &tools,
        &[],
        build_file_change_executor(&tools.file_write),
        vec![Arc::new(AgentTaskTool::new(agent_service))],
    );
    let profiles = ProfileRegistry::single(
        "default",
        llm.clone(),
        TerminationCapability::summary_prompt_only(),
    );
    let (rpc, hook) = basic_pack_arc();
    let service = RequestService::new(
        profiles,
        registry,
        tools.clone(),
        Arc::new(ToolRoundTerminatorOrchestrator::new(
            tools.termination_strategy,
        )),
        "default".into(),
        Arc::new(ConversationStore::new(dir.path().join("conversations"))),
        StaticCapabilityPolicy::local_full(),
        rpc,
        hook,
        FeatureRegistry::empty(),
    );
    let approval = Arc::new(Approval {
        calls: AtomicUsize::new(0),
    });
    let response = service
        .handle_with_events(
            ClientRequest::AgentTurn {
                id: "0070-vertical".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "delegate and verify artifact".into(),
                }],
                tools: vec![
                    AGENT_TASK.into(),
                    SHELL_EXEC.into(),
                    READ_FILE.into(),
                    GIT_DIFF.into(),
                ],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some(dir.path().display().to_string()),
                    task_completion: true,
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
            Some(approval.clone()),
            None,
            None,
            None,
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult {
        status,
        tool_calls,
        completion_report: Some(report),
        ..
    } = response
    else {
        panic!("unexpected response: {response:?}");
    };
    assert_eq!(status, AgentTurnStatus::Ok);
    assert_eq!(report.outcome, WireOutcome::Done);
    assert_eq!(
        report.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Done)
    );
    assert_eq!(report.queries_used, 2);
    assert_eq!(report.follow_up_count, Some(1));
    assert_eq!(report.worker_id.as_deref(), Some("fixture"));
    assert_eq!(report.gaps.len(), 1);
    assert_eq!(report.gaps[0].criterion_id, "c1");
    assert_eq!(
        report.gaps[0].verification_plan_item_ids,
        vec![
            "v-command".to_string(),
            "v-read".to_string(),
            "v-diff".to_string()
        ]
    );
    assert_eq!(
        tool_calls
            .iter()
            .filter(|call| call.name == AGENT_TASK)
            .count(),
        2
    );
    assert_eq!(
        tool_calls
            .iter()
            .filter(|call| call.name == GIT_DIFF)
            .count(),
        2
    );
    assert_eq!(
        tool_calls
            .iter()
            .filter(|call| call.name == READ_FILE)
            .count(),
        2
    );
    assert!(report.criteria[0].evidence.iter().any(|item| item.source
        == aibe_protocol::CompletionEvidenceSource::Verification
        && item.verified));
    let worker_results = tool_calls
        .iter()
        .filter(|call| call.name == AGENT_TASK)
        .map(|call| {
            call.output
                .as_deref()
                .and_then(|raw| serde_json::from_str::<AgentTaskResult>(raw).ok())
                .expect("structured AgentTaskResult")
        })
        .collect::<Vec<_>>();
    assert!(worker_results
        .iter()
        .all(|result| { !result.verified && result.evidence.iter().all(|item| !item.verified) }));
    assert!(worker_results.iter().all(|result| {
        result
            .changed_paths
            .iter()
            .any(|path| path == Path::new("0070-artifact.txt"))
    }));
    assert_eq!(approval.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("0070-artifact.txt"))
            .expect("artifact")
            .trim(),
        "verified"
    );
    assert_eq!(
        llm.recorded_calls().len(),
        10,
        "two bounded outer queries, five tool rounds each"
    );
}

#[test]
fn parent_contract_owns_delegated_completion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract = contract(dir.path());
    let result = AgentTaskResult::unverified(
        AgentTaskStatus::Completed,
        "done",
        true,
        vec![],
        String::new(),
        String::new(),
        false,
        false,
        Some(0),
        false,
        vec!["0070-artifact.txt".into()],
        false,
        vec![],
        "explicit_ui",
        "fixture",
        dir.path().display().to_string(),
        3,
    );
    assert!(!result.verified);
    assert!(result.evidence.iter().all(|item| !item.verified));
    let call = ExecutedToolCall::ok(
        "a1".into(),
        AGENT_TASK,
        serde_json::to_value(initial_request()).expect("args"),
        serde_json::to_string(&result).expect("result"),
    );
    let evidence = evidence_from_tools(&contract, &[call]);
    assert_eq!(evidence[0].source, EvidenceSource::AgentTask);
    assert!(!evidence[0].verified);
    assert!(build_report(
        &contract,
        &evidence,
        &evaluation(CriterionStatus::Satisfied, &["e1"], None),
        1,
        false
    )
    .is_err());
}

#[test]
fn parent_reobserves_artifacts_and_external_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract = contract(dir.path());
    let gate = ContractGate::strict(
        TaskCompletionEligibility::Active {
            expected_kind: TaskKind::Execution,
        },
        "delegate work",
    );
    let bad = ToolCall {
        id: "bad".into(),
        name: AGENT_TASK.into(),
        arguments: json!({"worker":"fixture","objective":"x","instructions":["x"],"completion_criteria":[{"id":"outside","description":"x"}],"timeout_secs":3}),
        provider_extras: None,
    };
    gate.inspect_before_tools(&envelope(&contract, None, ""), true)
        .expect("fix contract");
    assert!(gate.inspect_tool_calls(&[bad], dir.path()).is_err());

    let policy_gate = ContractGate::strict_with_delegated_policy(
        TaskCompletionEligibility::Active {
            expected_kind: TaskKind::Execution,
        },
        "delegate work",
        [
            AGENT_TASK.to_string(),
            SHELL_EXEC.to_string(),
            READ_FILE.to_string(),
            GIT_DIFF.to_string(),
        ],
        vec![],
    );
    policy_gate
        .inspect_before_tools(&envelope(&contract, None, ""), true)
        .expect("fix contract");
    let valid_agent = ToolCall {
        id: "valid-agent".into(),
        name: AGENT_TASK.into(),
        arguments: serde_json::to_value(initial_request()).expect("request"),
        provider_extras: None,
    };
    assert!(policy_gate
        .inspect_tool_calls(&[valid_agent], dir.path())
        .unwrap_err()
        .contains("command is not allowed"));

    let exact = ExecutedToolCall::ok(
        "v1".into(),
        SHELL_EXEC,
        json!({"command":"test","args":["-f","0070-artifact.txt"]}),
        String::new(),
    )
    .with_audit(
        ToolRiskClass::DangerousShell,
        ToolApprovalState::ExplicitClientOptIn,
        false,
    );
    let mismatch = ExecutedToolCall::ok(
        "v2".into(),
        SHELL_EXEC,
        json!({"command":"test","args":["-e","0070-artifact.txt"]}),
        String::new(),
    )
    .with_audit(
        ToolRiskClass::DangerousShell,
        ToolApprovalState::ExplicitClientOptIn,
        false,
    );
    let exact_evidence = evidence_from_tools(&contract, &[exact.clone()]);
    assert_eq!(exact_evidence[0].source, EvidenceSource::Verification);
    assert!(exact_evidence[0].verified);
    let mismatch_evidence = evidence_from_tools(&contract, &[mismatch]);
    assert_eq!(
        mismatch_evidence[0].source,
        EvidenceSource::UnknownShellEffect
    );
    assert!(!mismatch_evidence[0].verified);

    let agent = ExecutedToolCall::ok(
        "agent".into(),
        AGENT_TASK,
        serde_json::to_value(initial_request()).expect("request"),
        r#"{"status":"done","verified":false,"cwd":"unused"}"#.into(),
    );
    let read = ExecutedToolCall::ok(
        "read".into(),
        READ_FILE,
        json!({"path":"0070-artifact.txt"}),
        "verified".into(),
    );
    let diff = ExecutedToolCall::ok(
        "diff".into(),
        GIT_DIFF,
        json!({"path":"0070-artifact.txt"}),
        "diff".into(),
    );
    assert!(validate_delegated_cycle_calls(
        &contract,
        &[agent.clone(), exact.clone(), read.clone(), diff.clone()]
    )
    .is_ok());
    // 逆順: observation が command より前だと fail-closed
    assert!(validate_delegated_cycle_calls(
        &contract,
        &[agent.clone(), read.clone(), exact.clone(), diff.clone()]
    )
    .is_err());
    // 同一 observation 実行を複数 plan item に再利用すると fail-closed
    assert!(
        validate_delegated_cycle_calls(&contract, &[agent, exact, read.clone(), read.clone()])
            .is_err()
    );

    // collaborative handoff は subprocess 未実行のため Verification に昇格しない
    let handoff = ExecutedToolCall::ok(
        "handoff".into(),
        SHELL_EXEC,
        json!({"command":"test","args":["-f","0070-artifact.txt"]}),
        String::new(),
    )
    .with_shell_exec_audit(
        "ask",
        ShellExecApprovalOutcome::CollaborativeHandoff,
        None,
        None,
    );
    let handoff_evidence = evidence_from_tools(&contract, &[handoff]);
    assert_eq!(
        handoff_evidence[0].source,
        EvidenceSource::UnknownShellEffect
    );
    assert!(!handoff_evidence[0].verified);

    // 非 zero exit は plan 未実行（InvalidRequest）ではなく Verification(verified=false)
    let failed_command = ExecutedToolCall::err(
        "v-fail".into(),
        SHELL_EXEC,
        json!({"command":"test","args":["-f","0070-artifact.txt"]}),
        "nonzero_exit",
        "exit status 1",
    )
    .with_audit(
        ToolRiskClass::DangerousShell,
        ToolApprovalState::ExplicitClientOptIn,
        false,
    );
    let failed_evidence = evidence_from_tools(&contract, &[failed_command.clone()]);
    assert_eq!(failed_evidence[0].source, EvidenceSource::Verification);
    assert!(!failed_evidence[0].verified);
    assert_eq!(
        failed_evidence[0].plan_item_id.as_deref(),
        Some("v-command")
    );
    assert!(validate_delegated_cycle_calls(
        &contract,
        &[
            ExecutedToolCall::ok(
                "agent-fail".into(),
                AGENT_TASK,
                serde_json::to_value(initial_request()).expect("request"),
                r#"{"status":"done","verified":false,"cwd":"unused"}"#.into(),
            ),
            failed_command,
            read.clone(),
            diff.clone(),
        ]
    )
    .is_ok());

    // MVP: command plan item は1件まで
    let mut multi_command = contract.clone();
    multi_command.delegated_verification = Some(DelegatedVerificationPlan {
        items: vec![
            DelegatedVerificationPlanItem {
                id: "v-command-a".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Command {
                    command: "test".into(),
                    args: vec!["-f".into(), "a.txt".into()],
                    cwd: dir.path().display().to_string(),
                },
                expected_success: "exit 0".into(),
            },
            DelegatedVerificationPlanItem {
                id: "v-command-b".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Command {
                    command: "test".into(),
                    args: vec!["-f".into(), "b.txt".into()],
                    cwd: dir.path().display().to_string(),
                },
                expected_success: "exit 0".into(),
            },
            DelegatedVerificationPlanItem {
                id: "v-read-only".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Observation {
                    tool: "read_file".into(),
                    target: "0070-artifact.txt".into(),
                },
                expected_success: "content".into(),
            },
        ],
    });
    assert!(multi_command
        .validate()
        .unwrap_err()
        .contains("at most one command item"));

    // Agent Task は cwd digest ではなく global prior effect として後続 observation を verified にする
    let agent_effect = ExecutedToolCall::ok(
        "agent-effect".into(),
        AGENT_TASK,
        serde_json::to_value(initial_request()).expect("request"),
        r#"{"status":"done","verified":false,"cwd":"/tmp/worker-cwd"}"#.into(),
    );
    let post_read = ExecutedToolCall::ok(
        "post-read".into(),
        READ_FILE,
        json!({"path":"0070-artifact.txt"}),
        "verified".into(),
    )
    .with_audit(
        ToolRiskClass::ReadOnly,
        ToolApprovalState::NotRequired,
        false,
    );
    let after_agent = append_evidence_from_tools(&contract, &[], &[agent_effect, post_read]);
    assert!(after_agent[0].target.is_none());
    assert_eq!(after_agent[0].source, EvidenceSource::AgentTask);
    assert_eq!(after_agent[1].source, EvidenceSource::Observation);
    assert!(after_agent[1].observed_after_effect);
    assert!(after_agent[1].verified);

    // plan に固定した grep は verification_tools 既定外でも verified になる
    let mut grep_contract = contract.clone();
    grep_contract.verification_tools = vec!["read_file".into()];
    grep_contract.delegated_verification = Some(DelegatedVerificationPlan {
        items: vec![
            DelegatedVerificationPlanItem {
                id: "v-command".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Command {
                    command: "test".into(),
                    args: vec!["-f".into(), "0070-artifact.txt".into()],
                    cwd: dir.path().display().to_string(),
                },
                expected_success: "exit status 0".into(),
            },
            DelegatedVerificationPlanItem {
                id: "v-grep".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Observation {
                    tool: "grep".into(),
                    target: "0070-artifact.txt".into(),
                },
                expected_success: "content matches".into(),
            },
        ],
    });
    let agent_for_grep = ExecutedToolCall::ok(
        "agent-grep".into(),
        AGENT_TASK,
        serde_json::to_value(initial_request()).expect("request"),
        r#"{"status":"done","verified":false}"#.into(),
    );
    let grep = ExecutedToolCall::ok(
        "grep".into(),
        "grep",
        json!({"path":"0070-artifact.txt","pattern":"verified"}),
        "verified".into(),
    )
    .with_audit(
        ToolRiskClass::ReadOnly,
        ToolApprovalState::NotRequired,
        false,
    );
    let grep_evidence = append_evidence_from_tools(&grep_contract, &[], &[agent_for_grep, grep]);
    assert_eq!(grep_evidence[1].source, EvidenceSource::Observation);
    assert_eq!(grep_evidence[1].plan_item_id.as_deref(), Some("v-grep"));
    assert!(grep_evidence[1].verified);
}

#[test]
fn evidence_precedence_and_conflicts_fail_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract = contract(dir.path());
    let records = vec![
        EvidenceRecord {
            evidence_id: "worker".into(),
            criterion_ids: vec!["c1".into()],
            source: EvidenceSource::AgentTask,
            observed_after_effect: false,
            summary: "worker says done".into(),
            verified: false,
            target: Some("same".into()),
            stale: false,
            plan_item_id: None,
            value_fingerprint: None,
        },
        EvidenceRecord {
            evidence_id: "direct".into(),
            criterion_ids: vec!["c1".into()],
            source: EvidenceSource::Observation,
            observed_after_effect: true,
            summary: "direct observation".into(),
            verified: true,
            target: Some("same".into()),
            stale: true,
            plan_item_id: None,
            value_fingerprint: None,
        },
    ];
    assert!(validate_evaluation(
        &contract,
        &records,
        &evaluation(CriterionStatus::Satisfied, &["worker"], None)
    )
    .is_err());
    assert!(validate_evaluation(
        &contract,
        &records,
        &evaluation(CriterionStatus::Satisfied, &["direct"], None)
    )
    .is_err());
    assert!(validate_evaluation(
        &contract,
        &records,
        &evaluation(CriterionStatus::Unknown, &[], Some("reobserve"))
    )
    .is_ok());

    let plan_records = vec![
        EvidenceRecord {
            evidence_id: "command".into(),
            criterion_ids: vec!["c1".into()],
            source: EvidenceSource::Verification,
            observed_after_effect: true,
            summary: "shell_exec status=Ok".into(),
            verified: true,
            target: Some("artifact".into()),
            stale: false,
            plan_item_id: Some("v-command".into()),
            value_fingerprint: Some("sha256:command".into()),
        },
        EvidenceRecord {
            evidence_id: "read-a".into(),
            criterion_ids: vec!["c1".into()],
            source: EvidenceSource::Observation,
            observed_after_effect: true,
            summary: "read_file status=Ok".into(),
            verified: true,
            target: Some("artifact".into()),
            stale: false,
            plan_item_id: Some("v-read".into()),
            value_fingerprint: Some("sha256:a".into()),
        },
        EvidenceRecord {
            evidence_id: "read-b".into(),
            criterion_ids: vec!["c1".into()],
            source: EvidenceSource::Observation,
            observed_after_effect: true,
            summary: "read_file status=Ok".into(),
            verified: true,
            target: Some("artifact".into()),
            stale: false,
            plan_item_id: Some("v-read".into()),
            value_fingerprint: Some("sha256:b".into()),
        },
        EvidenceRecord {
            evidence_id: "diff".into(),
            criterion_ids: vec!["c1".into()],
            source: EvidenceSource::Observation,
            observed_after_effect: true,
            summary: "git_diff status=Ok".into(),
            verified: true,
            target: Some("artifact".into()),
            stale: false,
            plan_item_id: Some("v-diff".into()),
            value_fingerprint: Some("sha256:diff".into()),
        },
    ];
    assert!(validate_evaluation(
        &contract,
        &plan_records,
        &evaluation(
            CriterionStatus::Satisfied,
            &["command", "read-a", "read-b", "diff"],
            None,
        ),
    )
    .unwrap_err()
    .contains("conflicting observation"));
    // 都合のよい片方だけを引用しても ledger 上の矛盾は隠せず fail-closed
    assert!(validate_evaluation(
        &contract,
        &plan_records,
        &evaluation(
            CriterionStatus::Satisfied,
            &["command", "read-a", "diff"],
            None,
        ),
    )
    .unwrap_err()
    .contains("conflicting observation"));

    // 同じ tool+target を別 plan item ID に分割しても矛盾は検出される
    let mut split_contract = contract.clone();
    split_contract.delegated_verification = Some(DelegatedVerificationPlan {
        items: vec![
            DelegatedVerificationPlanItem {
                id: "v-command".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Command {
                    command: "test".into(),
                    args: vec!["-f".into(), "0070-artifact.txt".into()],
                    cwd: dir.path().display().to_string(),
                },
                expected_success: "exit status 0".into(),
            },
            DelegatedVerificationPlanItem {
                id: "v-read-a".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Observation {
                    tool: "read_file".into(),
                    target: "0070-artifact.txt".into(),
                },
                expected_success: "content a".into(),
            },
            DelegatedVerificationPlanItem {
                id: "v-read-b".into(),
                criterion_ids: vec!["c1".into()],
                action: DelegatedVerificationAction::Observation {
                    tool: "read_file".into(),
                    target: "0070-artifact.txt".into(),
                },
                expected_success: "content b".into(),
            },
        ],
    });
    assert!(split_contract
        .validate()
        .unwrap_err()
        .contains("duplicate observation identity"));
    // 壊れた plan を evaluation に載せても Contract 検査で拒否される
    assert!(validate_evaluation(
        &split_contract,
        &[],
        &evaluation(CriterionStatus::Satisfied, &["command"], None),
    )
    .unwrap_err()
    .contains("duplicate observation identity"));
}

#[test]
fn unknown_not_applicable_require_delegated_plan() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plain = contract(dir.path());
    plain.delegated_verification = None;
    assert!(validate_evaluation(
        &plain,
        &[],
        &evaluation(CriterionStatus::Unknown, &[], Some("need more"))
    )
    .unwrap_err()
    .contains("only valid with delegated verification"));
    assert!(validate_evaluation(
        &plain,
        &[],
        &evaluation(CriterionStatus::NotApplicable, &[], None)
    )
    .unwrap_err()
    .contains("only valid with delegated verification"));

    let mut unknown = evaluation(CriterionStatus::Unknown, &[], None);
    unknown.next_objective = None;
    unknown.blocked = Some("cannot decide yet".into());
    let report = build_report(&contract(dir.path()), &[], &unknown, 2, false).expect("report");
    assert_eq!(
        report.criteria[0].evaluation_status,
        Some(aibe_protocol::CompletionCriterionStatus::Unknown)
    );
    assert!(report.unsatisfied_criteria.contains(&"c1".into()));
    assert!(report
        .unverified_items
        .iter()
        .any(|item| item.contains("unknown")));
}

#[test]
fn criterion_evaluation_is_exhaustive_and_structured() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut contract = contract(dir.path());
    contract.criteria[0].applicability = Some("only when artifact is requested".into());
    let evidence = vec![EvidenceRecord {
        evidence_id: "a1".into(),
        criterion_ids: vec!["c1".into()],
        source: EvidenceSource::Observation,
        observed_after_effect: true,
        summary: "condition false".into(),
        verified: true,
        target: None,
        stale: false,
        plan_item_id: None,
        value_fingerprint: None,
    }];
    let mut not_applicable = evaluation(CriterionStatus::NotApplicable, &[], None);
    not_applicable.criteria[0].applicability_evidence_ids = vec!["a1".into()];
    assert!(validate_evaluation(&contract, &evidence, &not_applicable).is_ok());
    assert_eq!(
        terminal_outcome(&not_applicable, 1, false),
        Some(CompletionOutcome::Done)
    );
    not_applicable.criteria[0]
        .applicability_evidence_ids
        .clear();
    assert!(validate_evaluation(&contract, &evidence, &not_applicable).is_err());
    let mut missing = not_applicable;
    missing.criteria.clear();
    assert!(validate_evaluation(&contract, &evidence, &missing).is_err());
}

#[test]
fn gap_follow_up_is_single_bounded_and_same_worker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract = contract(dir.path());
    let gap = build_gap(
        &contract,
        &evaluation(CriterionStatus::Unsatisfied, &[], Some("repair artifact")),
    )
    .expect("gap");
    assert_eq!(gap.entries.len(), 1);
    let follow_up = gap_follow_up_request(&contract, &initial_request(), &gap).expect("follow-up");
    assert_eq!(follow_up.worker, initial_request().worker);
    assert_eq!(follow_up.cwd, initial_request().cwd);
    assert_eq!(follow_up.timeout_secs, initial_request().timeout_secs);
    assert_eq!(follow_up.completion_criteria[0].id, "c1");
    assert!(follow_up
        .instructions
        .iter()
        .any(|item| item.contains("Gap c1")));

    let gate = ContractGate::strict(
        TaskCompletionEligibility::Active {
            expected_kind: TaskKind::Execution,
        },
        "delegate work",
    );
    gate.inspect_before_tools(&envelope(&contract, None, ""), true)
        .expect("fix contract");
    gate.inspect_tool_calls(
        &[ToolCall {
            id: "initial".into(),
            name: AGENT_TASK.into(),
            arguments: serde_json::to_value(initial_request()).expect("request"),
            provider_extras: None,
        }],
        dir.path(),
    )
    .expect("initial agent task");
    gate.expect_follow_up(follow_up.clone())
        .expect("fix follow-up");
    let mut tampered = follow_up;
    tampered.objective.push_str(" and do unrelated work");
    assert!(gate
        .inspect_tool_calls(
            &[ToolCall {
                id: "tampered".into(),
                name: AGENT_TASK.into(),
                arguments: serde_json::to_value(tampered).expect("request"),
                provider_extras: None,
            }],
            dir.path(),
        )
        .unwrap_err()
        .contains("differs from the fixed Gap request"));
}

#[test]
fn non_done_keeps_current_gap_while_done_audits_previous() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut contract = contract(dir.path());
    contract.criteria.push(CompletionCriterion {
        id: "c2".into(),
        description: "second criterion remains open".into(),
        deliverable_is_plan: false,
        observes_targets: vec![],
        applicability: None,
    });
    for item in &mut contract.delegated_verification.as_mut().unwrap().items {
        item.criterion_ids.push("c2".into());
    }

    let previous = CompletionEvaluation {
        criteria: vec![
            CriterionEvaluation {
                criterion_id: "c1".into(),
                status: CriterionStatus::Unsatisfied,
                evidence_ids: vec![],
                required_evidence: vec!["fix c1".into()],
                applicability_evidence_ids: vec![],
            },
            CriterionEvaluation {
                criterion_id: "c2".into(),
                status: CriterionStatus::Unknown,
                evidence_ids: vec![],
                required_evidence: vec!["observe c2".into()],
                applicability_evidence_ids: vec![],
            },
        ],
        next_objective: Some("repair c1".into()),
        needs_user: None,
        blocked: None,
        failure: None,
    };
    let current = CompletionEvaluation {
        criteria: vec![
            CriterionEvaluation {
                criterion_id: "c1".into(),
                status: CriterionStatus::Unknown,
                evidence_ids: vec![],
                required_evidence: vec!["recheck c1".into()],
                applicability_evidence_ids: vec![],
            },
            CriterionEvaluation {
                criterion_id: "c2".into(),
                status: CriterionStatus::Unsatisfied,
                evidence_ids: vec![],
                required_evidence: vec!["fix c2".into()],
                applicability_evidence_ids: vec![],
            },
        ],
        next_objective: None,
        needs_user: None,
        blocked: Some("still incomplete".into()),
        failure: None,
    };
    let report = build_report(&contract, &[], &current, 2, true).expect("current unfinished");
    assert!(!matches!(report.outcome, WireOutcome::Done));
    assert!(report.gaps.iter().any(|gap| gap.criterion_id == "c2"));
    // previous の c1 Gap で上書きされていないこと（c2 が残る）
    assert!(report.gaps.iter().any(|gap| gap.criterion_id == "c2"));

    let mut done_report = CompletionReport {
        outcome: WireOutcome::Done,
        terminal_reason: None,
        criteria: vec![],
        unsatisfied_criteria: vec![],
        unverified_items: vec![],
        queries_used: 2,
        verification_terminal: Some(aibe_protocol::VerificationTerminal::Done),
        gaps: vec![],
        worker_id: None,
        follow_up_count: Some(1),
    };
    attach_gap_report(&mut done_report, &contract, &previous);
    assert!(done_report.gaps.iter().any(|gap| gap.criterion_id == "c1"));
    assert!(done_report.gaps.iter().any(|gap| gap.criterion_id == "c2"));
}

#[test]
fn follow_up_repeats_verification_and_detects_stagnation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract = contract(dir.path());
    let cycle = |suffix: &str| {
        vec![
            ExecutedToolCall::ok(
                format!("agent-{suffix}"),
                AGENT_TASK,
                serde_json::to_value(initial_request()).expect("request"),
                json!({"cwd":dir.path().display().to_string(),"verified":false}).to_string(),
            ),
            ExecutedToolCall::ok(
                format!("command-{suffix}"),
                SHELL_EXEC,
                json!({"command":"test","args":["-f","0070-artifact.txt"]}),
                String::new(),
            )
            .with_audit(
                ToolRiskClass::DangerousShell,
                ToolApprovalState::ExplicitClientOptIn,
                false,
            ),
            ExecutedToolCall::ok(
                format!("read-{suffix}"),
                READ_FILE,
                json!({"path":"0070-artifact.txt"}),
                "same artifact".into(),
            )
            .with_audit(
                ToolRiskClass::ReadOnly,
                ToolApprovalState::NotRequired,
                false,
            ),
            ExecutedToolCall::ok(
                format!("diff-{suffix}"),
                GIT_DIFF,
                json!({"path":"0070-artifact.txt"}),
                "same diff".into(),
            )
            .with_audit(
                ToolRiskClass::ReadOnly,
                ToolApprovalState::NotRequired,
                false,
            ),
        ]
    };
    let first_evidence = evidence_from_tools(&contract, &cycle("first"));
    let second_evidence = append_evidence_from_tools(&contract, &first_evidence, &cycle("second"));
    let evaluation = evaluation(CriterionStatus::Unsatisfied, &[], Some("retry"));
    let previous = progress_snapshot(&evaluation, &first_evidence);
    let current = progress_snapshot(&evaluation, &second_evidence);
    assert!(is_stalled(&previous, &current));
    let report =
        build_report(&contract, &second_evidence, &evaluation, 2, true).expect("stagnated report");
    assert_eq!(
        report.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Stagnated)
    );
    assert_eq!(
        verification_terminal(
            &evaluation,
            VerificationTerminalInput {
                cancelled: false,
                verification_failed: false,
                follow_up_used: true,
                stalled: true,
                budget_exhausted: true
            }
        ),
        Some(VerificationTerminal::Stagnated)
    );
    let mut needs_user = evaluation;
    needs_user.next_objective = None;
    needs_user.needs_user = Some("manual approval required".into());
    assert_eq!(
        verification_terminal(
            &needs_user,
            VerificationTerminalInput {
                cancelled: false,
                verification_failed: false,
                follow_up_used: true,
                stalled: true,
                budget_exhausted: true
            }
        ),
        Some(VerificationTerminal::NeedsUser)
    );
}

#[test]
fn verification_terminal_outcomes_are_distinct() {
    let unfinished = evaluation(CriterionStatus::Unsatisfied, &[], Some("continue"));
    let done = evaluation(CriterionStatus::Satisfied, &[], None);
    let base = VerificationTerminalInput {
        cancelled: false,
        verification_failed: false,
        follow_up_used: false,
        stalled: false,
        budget_exhausted: false,
    };
    assert_eq!(
        verification_terminal(&done, base),
        Some(VerificationTerminal::Done)
    );
    assert_eq!(
        verification_terminal(
            &unfinished,
            VerificationTerminalInput {
                cancelled: true,
                ..base
            }
        ),
        Some(VerificationTerminal::Cancelled)
    );
    assert_eq!(
        verification_terminal(
            &unfinished,
            VerificationTerminalInput {
                verification_failed: true,
                ..base
            }
        ),
        Some(VerificationTerminal::Failed)
    );
    let mut needs = unfinished.clone();
    needs.next_objective = None;
    needs.needs_user = Some("input".into());
    assert_eq!(
        verification_terminal(&needs, base),
        Some(VerificationTerminal::NeedsUser)
    );
    let mut blocked = unfinished.clone();
    blocked.next_objective = None;
    blocked.blocked = Some("constraint".into());
    assert_eq!(
        verification_terminal(&blocked, base),
        Some(VerificationTerminal::Blocked)
    );
    assert_eq!(
        verification_terminal(
            &unfinished,
            VerificationTerminalInput {
                follow_up_used: true,
                stalled: true,
                ..base
            }
        ),
        Some(VerificationTerminal::Stagnated)
    );
    assert_eq!(
        verification_terminal(
            &unfinished,
            VerificationTerminalInput {
                budget_exhausted: true,
                ..base
            }
        ),
        Some(VerificationTerminal::BudgetExhausted)
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let contract = contract(dir.path());
    let calls = vec![
        ExecutedToolCall::ok(
            "agent".into(),
            AGENT_TASK,
            serde_json::to_value(initial_request()).expect("request"),
            json!({"cwd":dir.path().display().to_string(),"verified":false}).to_string(),
        ),
        ExecutedToolCall::ok(
            "command".into(),
            SHELL_EXEC,
            json!({"command":"test","args":["-f","0070-artifact.txt"]}),
            String::new(),
        )
        .with_audit(
            ToolRiskClass::DangerousShell,
            ToolApprovalState::ExplicitClientOptIn,
            false,
        ),
        ExecutedToolCall::ok(
            "read".into(),
            READ_FILE,
            json!({"path":"0070-artifact.txt"}),
            "verified".into(),
        )
        .with_audit(
            ToolRiskClass::ReadOnly,
            ToolApprovalState::NotRequired,
            false,
        ),
        ExecutedToolCall::ok(
            "diff".into(),
            GIT_DIFF,
            json!({"path":"0070-artifact.txt"}),
            "diff".into(),
        )
        .with_audit(
            ToolRiskClass::ReadOnly,
            ToolApprovalState::NotRequired,
            false,
        ),
    ];
    let evidence = evidence_from_tools(&contract, &calls);
    let done_report = build_report(
        &contract,
        &evidence,
        &evaluation(CriterionStatus::Satisfied, &["e2", "e3", "e4"], None),
        1,
        false,
    )
    .expect("done report");
    assert_eq!(done_report.outcome, WireOutcome::Done);
    assert_eq!(
        done_report.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Done)
    );

    let mut needs = unfinished.clone();
    needs.next_objective = None;
    needs.needs_user = Some("input".into());
    let needs_report = build_report(&contract, &[], &needs, 1, false).expect("needs report");
    assert_eq!(needs_report.outcome, WireOutcome::NeedsUser);
    let mut blocked = unfinished.clone();
    blocked.next_objective = None;
    blocked.blocked = Some("constraint".into());
    let blocked_report = build_report(&contract, &[], &blocked, 1, false).expect("blocked report");
    assert_eq!(blocked_report.outcome, WireOutcome::Blocked);
    assert_eq!(
        blocked_report.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Blocked)
    );
    let stagnated = build_report(&contract, &[], &unfinished, 2, true).expect("stagnated");
    assert_eq!(stagnated.outcome, WireOutcome::Blocked);
    assert_eq!(
        stagnated.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Stagnated)
    );
    let budget = build_report(&contract, &[], &unfinished, 2, false).expect("budget");
    assert_eq!(budget.outcome, WireOutcome::BudgetExhausted);
    assert_eq!(
        budget.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::BudgetExhausted)
    );

    let legacy: aibe_protocol::CompletionReport = serde_json::from_value(json!({
        "outcome":"done",
        "terminal_reason":null,
        "criteria":[],
        "unsatisfied_criteria":[],
        "unverified_items":[],
        "queries_used":1
    }))
    .expect("legacy completion report remains decodable");
    assert!(legacy.verification_terminal.is_none());
}

#[tokio::test]
async fn verification_preserves_existing_boundaries_and_human_task() {
    assert_eq!(TASK_COMPLETION_QUERY_BUDGET, 2);
    assert_eq!(DelegationDepth::delegated().permits_delegation(), false);
    let encoded = serde_json::to_value(initial_request()).expect("request");
    assert!(
        encoded.get("gap").is_none(),
        "0069 wire schema remains unchanged"
    );
    assert!(serde_json::from_value::<AgentTaskRequest>({
        let mut value = encoded;
        value["gap"] = json!({});
        value
    })
    .is_err());
    let cancelled = ClientResponse::Cancelled {
        id: "id".into(),
        turn_id: "turn".into(),
        reason: Some("cancel".into()),
    };
    let failed = ClientResponse::error("id".into(), ErrorCode::ToolError, "verification failed");
    assert!(matches!(cancelled, ClientResponse::Cancelled { .. }));
    assert!(matches!(failed, ClientResponse::Error { .. }));

    let dir = tempfile::tempdir().expect("tempdir");
    let mut human_contract = contract(dir.path());
    human_contract.delegated_verification = None;
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::text_only(envelope(
            &human_contract,
            Some(evaluation(
                CriterionStatus::Unsatisfied,
                &[],
                Some("ask a human"),
            )),
            "verification pending",
        )),
        LlmStepResult::with_tool_calls(
            envelope(&human_contract, None, ""),
            vec![ToolCall {
                id: "human".into(),
                name: HUMAN_TASK.into(),
                arguments: json!({"objective":"verify artifact manually"}),
                provider_extras: None,
            }],
        ),
    ]));
    let tools = ToolsConfig::default();
    let (rpc, hook) = basic_pack_arc();
    let service = RequestService::new(
        ProfileRegistry::single(
            "default",
            llm.clone(),
            TerminationCapability::summary_prompt_only(),
        ),
        aibe::application::build_default_tool_registry(&tools, &[]),
        tools.clone(),
        Arc::new(ToolRoundTerminatorOrchestrator::new(
            tools.termination_strategy,
        )),
        "default".into(),
        Arc::new(ConversationStore::new(dir.path().join("conversations"))),
        StaticCapabilityPolicy::local_full(),
        rpc,
        hook,
        FeatureRegistry::empty(),
    );
    let response = service
        .handle_with_events(
            ClientRequest::AgentTurn {
                id: "0070-human-regression".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "create and verify artifact".into(),
                }],
                tools: vec![SHELL_EXEC.into(), HUMAN_TASK.into()],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some(dir.path().display().to_string()),
                    execution_mode: ExecutionMode::Collaborative,
                    task_completion: true,
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
            None,
            None,
            Some(Arc::new(SuspendedHumanTaskGate)),
            None,
            None,
        )
        .await;
    assert!(matches!(
        response,
        ClientResponse::AgentTurnResult {
            status: AgentTurnStatus::Suspended,
            completion_report: None,
            ..
        }
    ));
    assert_eq!(llm.recorded_calls().len(), 2);
}
