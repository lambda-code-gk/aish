//! Acceptance tests for spec 0071 Task Completion hardening.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aibe::adapters::outbound::agent_task::DefaultAgentTaskWorkerRegistry;
use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry_with_extras;
use aibe::adapters::outbound::{ConversationStore, ScriptedMockLlm, StaticCapabilityPolicy};
use aibe::application::agent_task::AgentTaskService;
use aibe::application::agent_task_tool::AgentTaskTool;
use aibe::application::{basic_pack_arc, build_file_change_executor, RequestService};
use aibe::domain::*;
use aibe::ports::outbound::*;
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, CompletionOutcome as WireOutcome,
    ExecutedToolStatus, ProtocolMessage, RequestContext, ToolApprovalOrigin,
};
use async_trait::async_trait;
use serde_json::json;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/0069_agent_task_worker.sh")
}

fn hardening_contract(cwd: &Path) -> TaskContract {
    TaskContract {
        goal: "delegate, repair, and independently verify two criteria".into(),
        task_kind: TaskKind::Execution,
        criteria: vec![
            CompletionCriterion {
                id: "c1".into(),
                description: "artifact passes the fixed verification command".into(),
                deliverable_is_plan: false,
                observes_targets: vec![],
                applicability: None,
            },
            CompletionCriterion {
                id: "c2".into(),
                description: "artifact content is independently observed".into(),
                deliverable_is_plan: false,
                observes_targets: vec![],
                applicability: None,
            },
        ],
        constraints: vec!["same worker and cwd".into()],
        deliverables: vec!["0070-artifact.txt".into()],
        verification: vec!["run the fixed command and observe the artifact".into()],
        verification_tools: vec!["read_file".into(), "git_diff".into()],
        delegated_verification: Some(DelegatedVerificationPlan {
            items: vec![
                DelegatedVerificationPlanItem {
                    id: "v-command".into(),
                    criterion_ids: vec!["c1".into()],
                    action: DelegatedVerificationAction::Command {
                        command: "grep".into(),
                        args: vec!["verified".into(), "0070-artifact.txt".into()],
                        cwd: cwd.display().to_string(),
                    },
                    expected_success: "exit status 0".into(),
                },
                DelegatedVerificationPlanItem {
                    id: "v-read".into(),
                    criterion_ids: vec!["c2".into()],
                    action: DelegatedVerificationAction::Observation {
                        tool: "read_file".into(),
                        target: "0070-artifact.txt".into(),
                    },
                    expected_success: "content is verified".into(),
                },
                DelegatedVerificationPlanItem {
                    id: "v-diff".into(),
                    criterion_ids: vec!["c2".into()],
                    action: DelegatedVerificationAction::Observation {
                        tool: "git_diff".into(),
                        target: "0070-artifact.txt".into(),
                    },
                    expected_success: "artifact diff is observed".into(),
                },
            ],
        }),
    }
}

fn initial_request() -> AgentTaskRequest {
    AgentTaskRequest {
        worker: WorkerId::parse("fixture").expect("worker"),
        objective: "write the artifact".into(),
        instructions: vec!["produce the requested artifact".into()],
        completion_criteria: vec![
            AgentTaskCriterion {
                id: "c1".into(),
                description: "artifact passes the fixed verification command".into(),
            },
            AgentTaskCriterion {
                id: "c2".into(),
                description: "artifact content is independently observed".into(),
            },
        ],
        cwd: None,
        timeout_secs: Some(3),
    }
}

fn hardening_evaluation(satisfied: bool, second_query: bool) -> CompletionEvaluation {
    let (command_evidence, content_evidence) = if second_query {
        (vec!["e6".into()], vec!["e7".into(), "e8".into()])
    } else {
        (vec!["e2".into()], vec!["e3".into(), "e4".into()])
    };
    CompletionEvaluation {
        criteria: vec![
            CriterionEvaluation {
                criterion_id: "c1".into(),
                status: if satisfied {
                    CriterionStatus::Satisfied
                } else {
                    CriterionStatus::Unsatisfied
                },
                evidence_ids: command_evidence,
                required_evidence: (!satisfied)
                    .then(|| vec!["successful fixed verification command".into()])
                    .unwrap_or_default(),
                applicability_evidence_ids: vec![],
                required_work: (!satisfied).then(|| "make the fixed command succeed".into()),
            },
            CriterionEvaluation {
                criterion_id: "c2".into(),
                status: if satisfied {
                    CriterionStatus::Satisfied
                } else {
                    CriterionStatus::Unsatisfied
                },
                evidence_ids: content_evidence,
                required_evidence: (!satisfied)
                    .then(|| vec!["fresh independent artifact observation".into()])
                    .unwrap_or_default(),
                applicability_evidence_ids: vec![],
                required_work: (!satisfied)
                    .then(|| "replace incomplete content with verified".into()),
            },
        ],
        next_objective: (!satisfied).then(|| "repair both criteria".into()),
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

struct Approval(AtomicUsize);

struct CancellingApproval {
    cancellation: Arc<aibe::application::TurnCancellation>,
}

#[async_trait]
impl ToolApprovalGate for CancellingApproval {
    async fn request_tool_approval(
        &self,
        _: &str,
        prompt: ToolApprovalPromptRequest,
    ) -> ToolApprovalGateOutcome {
        assert_eq!(prompt.tool_name, AGENT_TASK);
        self.cancellation.cancel();
        std::future::pending().await
    }
}

#[derive(Default)]
struct RecordingEvents {
    deltas: tokio::sync::Mutex<Vec<String>>,
    progress_count: AtomicUsize,
    final_count: AtomicUsize,
}

#[async_trait]
impl TurnEventSink for RecordingEvents {
    async fn progress(
        &self,
        _id: &str,
        _phase: aibe_protocol::ProgressPhase,
        _message: Option<String>,
    ) {
        self.progress_count.fetch_add(1, Ordering::SeqCst);
    }

    async fn assistant_streaming(&self, _id: &str, delta: String) {
        self.deltas.lock().await.push(delta);
    }

    async fn final_response(&self, _id: &str) {
        self.final_count.fetch_add(1, Ordering::SeqCst);
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
        self.0.fetch_add(1, Ordering::SeqCst);
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes)
    }
}

fn production_service(
    dir: &Path,
    llm: Arc<ScriptedMockLlm>,
    allowed_commands: Vec<String>,
) -> RequestService {
    let workers = DefaultAgentTaskWorkerRegistry::from_configs(&[AgentTaskWorkerConfig {
        id: "fixture".into(),
        executable: fixture(),
        args: vec!["gap_repair".into()],
        timeout_secs: 3,
        permission_profile: "test-bounded".into(),
        env_allowlist: vec![],
    }])
    .expect("workers");
    let agent_service = Arc::new(AgentTaskService::new(
        Arc::new(workers),
        true,
        vec![dir.to_path_buf()],
        8192,
        1800,
    ));
    let tools = ToolsConfig {
        shell_exec: ShellExecConfig {
            enabled: true,
            allowed_commands,
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        },
        read_file: ReadFileConfig {
            allowed_roots: vec![dir.to_path_buf()],
        },
        file_write: FileWriteConfig {
            allowed_roots: vec![dir.to_path_buf()],
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
    let profiles =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let (rpc, hook) = basic_pack_arc();
    RequestService::new(
        profiles,
        registry,
        tools.clone(),
        Arc::new(ToolRoundTerminatorOrchestrator::new(
            tools.termination_strategy,
        )),
        "default".into(),
        Arc::new(ConversationStore::new(dir.join("conversations"))),
        StaticCapabilityPolicy::local_full(),
        rpc,
        hook,
        FeatureRegistry::empty(),
    )
}

fn active_request(id: &str, dir: &Path, tools: Vec<String>) -> ClientRequest {
    ClientRequest::AgentTurn {
        id: id.into(),
        messages: vec![ProtocolMessage {
            role: "user".into(),
            content: "delegate and verify artifact".into(),
        }],
        tools,
        client_tools: vec![],
        context: RequestContext {
            cwd: Some(dir.display().to_string()),
            task_completion: true,
            ..Default::default()
        },
        llm_profile: None,
    }
}

fn first_gap_evidence() -> Vec<EvidenceRecord> {
    vec![
        EvidenceRecord {
            evidence_id: "e2".into(),
            criterion_ids: vec!["c1".into()],
            source: EvidenceSource::Verification,
            observed_after_effect: true,
            summary: "shell_exec status=Error".into(),
            verified: false,
            target: None,
            stale: false,
            plan_item_id: Some("v-command".into()),
            value_fingerprint: None,
        },
        EvidenceRecord {
            evidence_id: "e3".into(),
            criterion_ids: vec!["c2".into()],
            source: EvidenceSource::Observation,
            observed_after_effect: true,
            summary: "read_file status=Ok".into(),
            verified: true,
            target: None,
            stale: false,
            plan_item_id: Some("v-read".into()),
            value_fingerprint: None,
        },
        EvidenceRecord {
            evidence_id: "e4".into(),
            criterion_ids: vec!["c2".into()],
            source: EvidenceSource::Observation,
            observed_after_effect: true,
            summary: "git_diff status=Ok".into(),
            verified: true,
            target: None,
            stale: false,
            plan_item_id: Some("v-diff".into()),
            value_fingerprint: None,
        },
    ]
}

#[tokio::test]
async fn task_completion_cancel_report_vertical_e2e() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let service = production_service(
        dir.path(),
        Arc::new(ScriptedMockLlm::new(vec![])),
        vec!["grep".into()],
    );
    let cancellation = Arc::new(aibe::application::TurnCancellation::new());
    cancellation.cancel();
    let response = service
        .handle_with_events(
            active_request("0071-cancel", dir.path(), vec![AGENT_TASK.into()]),
            None,
            None,
            None,
            None,
            None,
            Some(cancellation),
        )
        .await;
    let ClientResponse::Cancelled {
        completion_report: Some(report),
        ..
    } = response
    else {
        panic!("unexpected response: {response:?}");
    };
    assert_eq!(
        report.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Cancelled)
    );
    let wire = serde_json::to_vec(&report).expect("report JSON");
    assert!(wire.len() <= aibe::application::task_completion::COMPLETION_REPORT_MAX_BYTES);
    assert!(!String::from_utf8_lossy(&wire).contains("aish_task_completion"));

    // Contract 固定後、agent_task approval 待ちの明示取消も同じ production
    // composition を通り、固定済み criterion を Unknown として投影する。
    let contract = hardening_contract(dir.path());
    let llm = Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::with_tool_calls(
        envelope(&contract, None, "must not be displayed"),
        vec![ToolCall {
            id: "cancel-after-contract".into(),
            name: AGENT_TASK.into(),
            arguments: serde_json::to_value(initial_request()).expect("request"),
            provider_extras: None,
        }],
    )]));
    let service = production_service(dir.path(), llm, vec!["grep".into()]);
    let cancellation = Arc::new(aibe::application::TurnCancellation::new());
    let cancelling_gate = Arc::new(CancellingApproval {
        cancellation: Arc::clone(&cancellation),
    });
    let response = service
        .handle_with_events(
            active_request(
                "0071-cancel-fixed",
                dir.path(),
                vec![
                    AGENT_TASK.into(),
                    SHELL_EXEC.into(),
                    READ_FILE.into(),
                    GIT_DIFF.into(),
                ],
            ),
            None,
            Some(cancelling_gate),
            None,
            None,
            None,
            Some(cancellation),
        )
        .await;
    let ClientResponse::Cancelled {
        completion_report: Some(report),
        ..
    } = response
    else {
        panic!("unexpected response: {response:?}");
    };
    assert_eq!(
        report.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Cancelled)
    );
    assert_eq!(report.criteria.len(), contract.criteria.len());
    assert!(report.criteria.iter().all(|criterion| {
        criterion.evaluation_status == Some(aibe_protocol::CompletionCriterionStatus::Unknown)
    }));
    assert!(
        serde_json::to_vec(&report).unwrap().len()
            <= aibe::application::task_completion::COMPLETION_REPORT_MAX_BYTES
    );
}

#[tokio::test]
async fn verification_infrastructure_failure_reports_failed() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let mut contract = hardening_contract(dir.path());
    let DelegatedVerificationAction::Command { command, args, .. } =
        &mut contract.delegated_verification.as_mut().unwrap().items[0].action
    else {
        unreachable!()
    };
    *command = "0071-missing-verifier".into();
    args.clear();
    let contract_only = || envelope(&contract, None, "");
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "worker-1".into(),
                name: AGENT_TASK.into(),
                arguments: serde_json::to_value(initial_request()).expect("request"),
                provider_extras: None,
            }],
        ),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "failed-verifier".into(),
                name: SHELL_EXEC.into(),
                arguments: json!({"command":"0071-missing-verifier","args":[]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only(envelope(&contract, None, "must not be displayed")),
    ]));
    let service = production_service(dir.path(), llm, vec!["0071-missing-verifier".into()]);
    let approval = Arc::new(Approval(AtomicUsize::new(0)));
    let response = service
        .handle_with_events(
            active_request(
                "0071-failed",
                dir.path(),
                vec![
                    AGENT_TASK.into(),
                    SHELL_EXEC.into(),
                    READ_FILE.into(),
                    GIT_DIFF.into(),
                ],
            ),
            None,
            Some(approval),
            None,
            None,
            None,
            None,
        )
        .await;
    let ClientResponse::Error {
        code,
        completion_report: Some(report),
        ..
    } = response
    else {
        panic!("unexpected response: {response:?}");
    };
    assert_eq!(code, aibe_protocol::ErrorCode::ToolError);
    assert_eq!(
        report.verification_terminal,
        Some(aibe_protocol::VerificationTerminal::Failed)
    );
    assert!(
        serde_json::to_vec(&report).unwrap().len()
            <= aibe::application::task_completion::COMPLETION_REPORT_MAX_BYTES
    );
}

#[tokio::test]
async fn task_completion_hardening_vertical_e2e() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let contract = hardening_contract(dir.path());
    let initial = initial_request();
    let contract_only = || envelope(&contract, None, "");
    let follow_up = AgentTaskRequest {
        worker: initial.worker.clone(),
        objective: format!(
            "{}; resolve the independently observed Gap",
            initial.objective
        ),
        instructions: vec![
            initial.instructions[0].clone(),
            "Gap c1: observed=e2: shell_exec status=Error; required_work=make the fixed command succeed; verify_with=v-command".into(),
            "Gap c2: observed=e3: read_file status=Ok; e4: git_diff status=Ok; required_work=replace incomplete content with verified; verify_with=v-read,v-diff".into(),
        ],
        completion_criteria: initial.completion_criteria.clone(),
        cwd: initial.cwd.clone(),
        timeout_secs: initial.timeout_secs,
    };
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
                arguments: json!({"command":"grep","args":["verified","0070-artifact.txt"]}),
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
            Some(hardening_evaluation(false, false)),
            "verification found two gaps",
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
                arguments: json!({"command":"grep","args":["verified","0070-artifact.txt"]}),
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
            Some(hardening_evaluation(true, true)),
            "independently verified",
        )),
    ]));
    let workers = DefaultAgentTaskWorkerRegistry::from_configs(&[AgentTaskWorkerConfig {
        id: "fixture".into(),
        executable: fixture(),
        args: vec!["gap_repair".into()],
        timeout_secs: 3,
        permission_profile: "test-bounded".into(),
        env_allowlist: vec![],
    }])
    .expect("workers");
    let agent_service = Arc::new(AgentTaskService::new(
        Arc::new(workers),
        true,
        vec![dir.path().to_path_buf()],
        8192,
        1800,
    ));
    let tools = ToolsConfig {
        shell_exec: ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["grep".into()],
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
    let approval = Arc::new(Approval(AtomicUsize::new(0)));
    let response = service
        .handle_with_events(
            ClientRequest::AgentTurn {
                id: "0071-vertical".into(),
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
    assert_eq!(report.queries_used, 2);
    assert_eq!(report.follow_up_count, Some(1));
    assert_eq!(report.worker_id.as_deref(), Some("fixture"));
    assert_eq!(report.gaps.len(), 2);
    assert_ne!(report.gaps[0].observed, report.gaps[1].observed);
    assert_ne!(report.gaps[0].required_work, report.gaps[1].required_work);
    let first_shell = tool_calls
        .iter()
        .find(|call| call.id == "check-1")
        .expect("first command");
    assert_eq!(first_shell.status, ExecutedToolStatus::Error);
    assert_eq!(
        first_shell.process_terminal,
        Some(aibe_protocol::ProcessExecutionTerminal::NonzeroExit)
    );
    assert_eq!(
        tool_calls
            .iter()
            .filter(|call| call.name == AGENT_TASK)
            .count(),
        2
    );
    assert_eq!(approval.0.load(Ordering::SeqCst), 2);
    assert_eq!(llm.recorded_calls().len(), 10);
}

#[test]
fn gap_observed_uses_actual_evidence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let evaluation = hardening_evaluation(false, false);
    let gap = build_gap(
        &hardening_contract(dir.path()),
        &first_gap_evidence(),
        &evaluation,
        &[],
    )
    .expect("Gap");
    assert_eq!(gap.entries[0].observed, "e2: shell_exec status=Error");
    assert_eq!(
        gap.entries[1].observed,
        "e3: read_file status=Ok; e4: git_diff status=Ok"
    );
    assert!(gap.entries.iter().all(|entry| {
        !entry
            .observed
            .contains("successful fixed verification command")
            && !entry
                .observed
                .contains("fresh independent artifact observation")
    }));
    let mut stale = first_gap_evidence();
    stale[0].stale = true;
    assert!(build_gap(&hardening_contract(dir.path()), &stale, &evaluation, &[]).is_err());
}

#[test]
fn gap_required_work_is_per_criterion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gap = build_gap(
        &hardening_contract(dir.path()),
        &first_gap_evidence(),
        &hardening_evaluation(false, false),
        &[],
    )
    .expect("Gap");
    assert_eq!(
        gap.entries[0].required_work,
        "make the fixed command succeed"
    );
    assert_eq!(
        gap.entries[1].required_work,
        "replace incomplete content with verified"
    );
    assert_ne!(gap.entries[0].required_work, gap.entries[1].required_work);
    let mut blank = hardening_evaluation(false, false);
    blank.criteria[0].required_work = Some("  ".into());
    assert!(build_gap(
        &hardening_contract(dir.path()),
        &first_gap_evidence(),
        &blank,
        &[],
    )
    .is_err());
    let mut oversized = hardening_evaluation(false, false);
    oversized.criteria[0].required_work = Some("x".repeat(1025));
    assert!(build_gap(
        &hardening_contract(dir.path()),
        &first_gap_evidence(),
        &oversized,
        &[],
    )
    .is_err());

    // Contract envelope 内には収まるが、plan ID が全 criterion の Gap に反復される
    // worst case でも report は criterion row を保持して128 KiB以内へ縮退する。
    let criterion_ids = (0..32).map(|index| format!("c{index}")).collect::<Vec<_>>();
    let large_contract = TaskContract {
        goal: "bounded report with repeated verification references".into(),
        task_kind: TaskKind::Execution,
        criteria: criterion_ids
            .iter()
            .map(|id| CompletionCriterion {
                id: id.clone(),
                description: format!("criterion {id}"),
                deliverable_is_plan: false,
                observes_targets: vec![],
                applicability: None,
            })
            .collect(),
        constraints: vec![],
        deliverables: vec!["artifact".into()],
        verification: vec!["observe fixed targets".into()],
        verification_tools: vec!["read_file".into()],
        delegated_verification: Some(DelegatedVerificationPlan {
            items: (0..32)
                .map(|index| DelegatedVerificationPlanItem {
                    id: format!("v{index:02}-{}", "x".repeat(235)),
                    criterion_ids: criterion_ids.clone(),
                    action: DelegatedVerificationAction::Observation {
                        tool: "read_file".into(),
                        target: format!("artifact-{index}"),
                    },
                    expected_success: "observed".into(),
                })
                .collect(),
        }),
    };
    let large_evaluation = CompletionEvaluation {
        criteria: criterion_ids
            .iter()
            .map(|id| CriterionEvaluation {
                criterion_id: id.clone(),
                status: CriterionStatus::Unknown,
                evidence_ids: vec![],
                required_evidence: vec![],
                applicability_evidence_ids: vec![],
                required_work: Some(format!("complete {id}")),
            })
            .collect(),
        next_objective: None,
        needs_user: None,
        blocked: Some("verification remains incomplete".into()),
        failure: None,
    };
    let report = aibe::application::task_completion::build_report(
        &large_contract,
        &[],
        &large_evaluation,
        1,
        false,
    )
    .expect("bounded report");
    assert_eq!(report.criteria.len(), 32);
    assert!(report.gaps.len() < 32);
    assert!(report
        .unverified_items
        .iter()
        .any(|item| item.contains("omitted")));
    assert!(
        serde_json::to_vec(&report).unwrap().len()
            <= aibe::application::task_completion::COMPLETION_REPORT_MAX_BYTES
    );
    let mut post_build_attachment = report.clone();
    post_build_attachment.gaps.clear();
    aibe::application::task_completion::attach_gap_report(
        &mut post_build_attachment,
        &large_contract,
        &[],
        &large_evaluation,
    );
    assert_eq!(post_build_attachment.criteria.len(), 32);
    assert!(
        serde_json::to_vec(&post_build_attachment).unwrap().len()
            <= aibe::application::task_completion::COMPLETION_REPORT_MAX_BYTES
    );
}

#[test]
fn gap_follow_up_request_is_per_criterion_and_bounded() {
    let dir = tempfile::tempdir().expect("tempdir");
    let contract = hardening_contract(dir.path());
    let gap = build_gap(
        &contract,
        &first_gap_evidence(),
        &hardening_evaluation(false, false),
        &[],
    )
    .expect("Gap");
    let request = gap_follow_up_request(&contract, &initial_request(), &gap).expect("follow-up");
    assert_eq!(request.instructions.len(), 3);
    assert!(request.instructions[1].contains("observed=e2: shell_exec status=Error"));
    assert!(request.instructions[1].contains("required_work=make the fixed command succeed"));
    assert!(request.instructions[1].contains("verify_with=v-command"));
    assert!(request.instructions[2].contains("verify_with=v-read,v-diff"));
    assert_eq!(request.completion_criteria.len(), 2);
    assert!(serde_json::to_vec(&request).unwrap().len() <= 128 * 1024);
    let mut over_instruction_bound = gap;
    over_instruction_bound.entries[0].observed = "o".repeat(1024);
    over_instruction_bound.entries[0].required_work = "w".repeat(1024);
    assert!(gap_follow_up_request(&contract, &initial_request(), &over_instruction_bound).is_err());
}

#[tokio::test]
async fn completion_event_buffer_has_total_byte_limit() {
    use aibe::application::task_completion::{
        CompletionEventBuffer, COMPLETION_EVENT_BUFFER_MAX_BYTES,
        COMPLETION_EVENT_BUFFER_MAX_DELTAS,
    };

    let events = Arc::new(RecordingEvents::default());
    let buffer = CompletionEventBuffer::new(events.clone());
    buffer
        .progress("turn", aibe_protocol::ProgressPhase::Thinking, None)
        .await;
    buffer
        .assistant_streaming("turn", "x".repeat(COMPLETION_EVENT_BUFFER_MAX_BYTES))
        .await;
    assert_eq!(
        buffer.buffered_usage().await,
        (COMPLETION_EVENT_BUFFER_MAX_BYTES, 1, false)
    );
    buffer
        .assistant_streaming("turn", "{\"aish_task_".into())
        .await;
    assert_eq!(buffer.buffered_usage().await, (0, 0, true));
    buffer
        .assistant_streaming("turn", "completion\":{}}".into())
        .await;
    buffer
        .flush_for_response(
            "turn",
            &ClientResponse::error(
                "turn".into(),
                aibe_protocol::ErrorCode::InvalidRequest,
                "fail closed",
            ),
        )
        .await;
    assert!(events.deltas.lock().await.is_empty());
    assert_eq!(events.progress_count.load(Ordering::SeqCst), 1);
    assert_eq!(events.final_count.load(Ordering::SeqCst), 1);
    assert_eq!(buffer.buffered_usage().await, (0, 0, false));

    let count_events = Arc::new(RecordingEvents::default());
    let count_buffer = CompletionEventBuffer::new(count_events);
    for _ in 0..=COMPLETION_EVENT_BUFFER_MAX_DELTAS {
        count_buffer
            .assistant_streaming("turn", String::new())
            .await;
    }
    assert_eq!(count_buffer.buffered_usage().await, (0, 0, true));

    let marker_events = Arc::new(RecordingEvents::default());
    let marker_buffer = CompletionEventBuffer::new(marker_events.clone());
    marker_buffer
        .assistant_streaming("turn", "{\"aish_task_".into())
        .await;
    marker_buffer
        .assistant_streaming("turn", "completion\":{}}".into())
        .await;
    marker_buffer
        .flush_for_response(
            "turn",
            &ClientResponse::AgentTurnResult {
                id: "turn".into(),
                status: AgentTurnStatus::Suspended,
                assistant_message: aibe_protocol::ProtocolMessageOut {
                    role: "assistant".into(),
                    content: String::new(),
                },
                tool_calls: vec![],
                completion_report: None,
            },
        )
        .await;
    assert!(marker_events.deltas.lock().await.is_empty());

    let deliverable_events = Arc::new(RecordingEvents::default());
    let deliverable_buffer = CompletionEventBuffer::new(deliverable_events.clone());
    deliverable_buffer
        .flush_for_response(
            "turn",
            &ClientResponse::AgentTurnResult {
                id: "turn".into(),
                status: AgentTurnStatus::Ok,
                assistant_message: aibe_protocol::ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "unexpected aish_task_completion control".into(),
                },
                tool_calls: vec![],
                completion_report: Some(
                    aibe::application::task_completion::build_interrupted_report(
                        None,
                        aibe_protocol::VerificationTerminal::Cancelled,
                        "cancelled",
                        1,
                    ),
                ),
            },
        )
        .await;
    assert!(deliverable_events.deltas.lock().await.is_empty());
}

#[test]
fn workspace_observer_detects_same_size_rewrite_or_marks_incomplete() {
    use aibe::adapters::outbound::agent_task::{observe_changes, snapshot_workspace};

    let dir = tempfile::tempdir().expect("tempdir");
    let artifact = dir.path().join("same-size.txt");
    std::fs::write(&artifact, b"aaaa").expect("initial content");
    let original_mtime = std::fs::metadata(&artifact)
        .expect("initial metadata")
        .modified()
        .expect("initial mtime");
    let before = snapshot_workspace(dir.path());
    std::fs::write(&artifact, b"bbbb").expect("same-size rewrite");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&artifact)
        .expect("open rewritten artifact")
        .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
        .expect("restore mtime");
    let after = snapshot_workspace(dir.path());
    let (changed, incomplete) = observe_changes(&before, &after);
    assert!(
        changed
            .iter()
            .any(|path| path == Path::new("same-size.txt"))
            || incomplete,
        "same-size rewrite must be observed or explicitly incomplete"
    );
}

#[tokio::test]
async fn task_completion_hardening_preserves_regressions() {
    #[derive(serde::Deserialize)]
    struct LegacyErrorShape {
        id: String,
        code: aibe_protocol::ErrorCode,
        message: String,
    }

    #[derive(serde::Deserialize)]
    struct LegacyCancelledShape {
        id: String,
        turn_id: String,
        reason: Option<String>,
    }

    assert_eq!(TASK_COMPLETION_QUERY_BUDGET, 2);

    let legacy_error = r#"{"type":"error","id":"e","code":"tool_error","message":"legacy"}"#;
    let decoded: ClientResponse = serde_json::from_str(legacy_error).expect("legacy Error");
    assert!(matches!(
        decoded,
        ClientResponse::Error {
            completion_report: None,
            ..
        }
    ));
    assert!(!serde_json::to_string(&decoded)
        .unwrap()
        .contains("completion_report"));

    let legacy_cancelled = r#"{"type":"cancelled","id":"c","turn_id":"t","reason":"legacy"}"#;
    let decoded: ClientResponse = serde_json::from_str(legacy_cancelled).expect("legacy Cancelled");
    assert!(matches!(
        decoded,
        ClientResponse::Cancelled {
            completion_report: None,
            ..
        }
    ));
    assert!(!serde_json::to_string(&decoded)
        .unwrap()
        .contains("completion_report"));

    let report = aibe::application::task_completion::build_interrupted_report(
        None,
        aibe_protocol::VerificationTerminal::Failed,
        "verification unavailable",
        1,
    );
    let new_error = ClientResponse::error_with_completion_report(
        "new-error".into(),
        aibe_protocol::ErrorCode::ToolError,
        "failed",
        report.clone(),
    );
    let legacy: LegacyErrorShape = serde_json::from_value(
        serde_json::to_value(new_error).expect("serialize new Error payload"),
    )
    .expect("legacy Error shape ignores additive completion_report");
    assert_eq!(legacy.id, "new-error");
    assert_eq!(legacy.code, aibe_protocol::ErrorCode::ToolError);
    assert_eq!(legacy.message, "failed");

    let new_cancelled = ClientResponse::Cancelled {
        id: "new-cancelled".into(),
        turn_id: "turn".into(),
        reason: Some("cancelled".into()),
        completion_report: Some(aibe_protocol::CompletionReport {
            verification_terminal: Some(aibe_protocol::VerificationTerminal::Cancelled),
            ..report
        }),
    };
    let legacy: LegacyCancelledShape = serde_json::from_value(
        serde_json::to_value(new_cancelled).expect("serialize new Cancelled payload"),
    )
    .expect("legacy Cancelled shape ignores additive completion_report");
    assert_eq!(legacy.id, "new-cancelled");
    assert_eq!(legacy.turn_id, "turn");
    assert_eq!(legacy.reason.as_deref(), Some("cancelled"));

    let suspended = ClientResponse::AgentTurnResult {
        id: "human".into(),
        status: AgentTurnStatus::Suspended,
        assistant_message: aibe_protocol::ProtocolMessageOut {
            role: "assistant".into(),
            content: "Human Task suspended".into(),
        },
        tool_calls: vec![],
        completion_report: None,
    };
    assert!(!serde_json::to_string(&suspended)
        .unwrap()
        .contains("completion_report"));

    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let service = production_service(
        dir.path(),
        Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::text_only(
            "ordinary mock answer",
        )])),
        vec![],
    );
    let response = service
        .handle_with_events(
            ClientRequest::AgentTurn {
                id: "0071-normal-regression".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "ordinary question".into(),
                }],
                tools: vec![],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some(dir.path().display().to_string()),
                    task_completion: false,
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult {
        status,
        assistant_message,
        completion_report,
        ..
    } = response
    else {
        panic!("unexpected response: {response:?}");
    };
    assert_eq!(status, AgentTurnStatus::Ok);
    assert_eq!(assistant_message.content, "ordinary mock answer");
    assert!(completion_report.is_none());
}

#[tokio::test]
async fn task_completion_hardening_verify_and_smoke() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let service = production_service(
        dir.path(),
        Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::text_only(
            "ordinary mock answer",
        )])),
        vec![],
    );
    let response = service
        .handle_with_events(
            ClientRequest::AgentTurn {
                id: "0071-normal-smoke".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "ordinary question".into(),
                }],
                tools: vec![],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some(dir.path().display().to_string()),
                    task_completion: false,
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult {
        status,
        assistant_message,
        completion_report,
        ..
    } = response
    else {
        panic!("unexpected response: {response:?}");
    };
    assert_eq!(status, AgentTurnStatus::Ok);
    assert_eq!(assistant_message.content, "ordinary mock answer");
    assert!(completion_report.is_none());
}
