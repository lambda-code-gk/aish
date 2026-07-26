//! Spec 0073 acceptance tests through the production request/turn/tool-round composition.

#![cfg(unix)]

use std::sync::{Arc, Mutex};

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::DefaultToolRegistry;
use aibe::adapters::outbound::{
    ConversationStore, GeminiLlm, OpenAiCompatibleLlm, ScriptedMockLlm, StaticCapabilityPolicy,
};
use aibe::application::completion_envelope::{
    decode_completion_envelope, CompletionEnvelopeDecode, ContractGate, ENVELOPE_MAX_BYTES,
};
use aibe::application::{basic_pack_arc, RequestService};
use aibe::domain::{
    ChatMessage, ExecutedToolCall, FeatureRegistry, LlmStepResult, MessageRole,
    TaskCompletionEligibility, TaskKind, ToolCall, ToolName, ToolResult, CONTRACT_TEXT_MAX_BYTES,
    READ_FILE, WRITE_FILE,
};
use aibe::ports::outbound::{
    LlmError, LlmProvider, ProfileRegistry, TerminationCapability, TerminationStrategy,
    ToolDefinition, ToolExecutionContext, ToolExecutor, ToolsConfig,
};
use aibe_protocol::{
    AgentTurnStatus, ClientProvidedToolSpec, ClientRequest, ClientResponse, CompletionOutcome,
    ErrorCode, ProtocolMessage, RequestContext, ToolRiskClass,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CONTROL: &str = "set_task_contract";

fn contract(goal: &str) -> Value {
    json!({
        "goal": goal,
        "task_kind": "execution",
        "criteria": [{
            "id": "c1",
            "description": "artifact is observed after creation",
            "deliverable_is_plan": false,
            "observes_targets": ["artifact.txt"]
        }],
        "constraints": ["request local"],
        "deliverables": ["artifact.txt"],
        "verification": ["read artifact.txt after creation"],
        "verification_tools": ["read_file"]
    })
}

fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
        provider_extras: None,
    }
}

fn control(id: &str, arguments: Value) -> ToolCall {
    call(id, CONTROL, arguments)
}

fn oversized_contract() -> Value {
    let mut value = contract("create artifact");
    value["constraints"] = json!(["x".repeat(ENVELOPE_MAX_BYTES)]);
    value
}

fn done(evidence_ids: &[&str]) -> String {
    json!({
        "aish_task_completion": {"evaluation": {"criteria": [{
            "criterion_id": "c1",
            "status": "satisfied",
            "evidence_ids": evidence_ids,
            "required_evidence": []
        }]}},
        "deliverable": "artifact verified"
    })
    .to_string()
}

fn blocked() -> String {
    json!({
        "aish_task_completion": {"evaluation": {
            "criteria": [{
                "criterion_id": "c1",
                "status": "unsatisfied",
                "evidence_ids": [],
                "required_evidence": ["observation"]
            }],
            "blocked": "fixture closed"
        }},
        "deliverable": "blocked"
    })
    .to_string()
}

#[derive(Default)]
struct ToolLog(Mutex<Vec<(String, String)>>);

struct RecordingTool {
    name: ToolName,
    log: Arc<ToolLog>,
}

#[async_trait]
impl ToolExecutor for RecordingTool {
    fn name(&self) -> ToolName {
        self.name.clone()
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        _timeout_ms: u64,
        _ctx: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult) {
        self.log
            .0
            .lock()
            .expect("tool log")
            .push((tool_call_id.into(), self.name.as_str().into()));
        let output = if self.name.as_str() == READ_FILE {
            "created\n"
        } else {
            "written"
        };
        (
            ExecutedToolCall::ok(
                tool_call_id.into(),
                self.name.as_str(),
                arguments.clone(),
                output.into(),
            ),
            ToolResult {
                tool_call_id: tool_call_id.into(),
                content: output.into(),
                is_error: false,
            },
        )
    }
}

struct Fixture {
    service: RequestService,
    llm: Arc<ScriptedMockLlm>,
    log: Arc<ToolLog>,
    store: Arc<ConversationStore>,
    _dir: tempfile::TempDir,
}

fn fixture(steps: Vec<LlmStepResult>) -> Fixture {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let log = Arc::new(ToolLog::default());
    let executors: Vec<Arc<dyn ToolExecutor>> = vec![
        Arc::new(RecordingTool {
            name: ToolName::write_file(),
            log: Arc::clone(&log),
        }),
        Arc::new(RecordingTool {
            name: ToolName::read_file(),
            log: Arc::clone(&log),
        }),
    ];
    let registry = Arc::new(DefaultToolRegistry::from_executors(executors).expect("registry"));
    let tools = ToolsConfig::default();
    let strategy = tools.termination_strategy;
    let store = Arc::new(ConversationStore::new(dir.path().join("conversations")));
    let profiles = ProfileRegistry::single(
        "default",
        llm.clone(),
        TerminationCapability::summary_prompt_only(),
    );
    let (rpc_extension, turn_hook) = basic_pack_arc();
    let service = RequestService::new(
        profiles,
        registry,
        tools,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".into(),
        store.clone(),
        StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    );
    Fixture {
        service,
        llm,
        log,
        store,
        _dir: dir,
    }
}

/// terminator が入力会話をそのまま応答へ引用する最悪ケースの provider。
/// `complete()` 入力を記録し、client 応答にも反映する。
struct QuotingTerminatorLlm {
    scripted: ScriptedMockLlm,
    terminator_input: Mutex<Option<Vec<ChatMessage>>>,
}

impl QuotingTerminatorLlm {
    fn new(steps: Vec<LlmStepResult>) -> Self {
        Self {
            scripted: ScriptedMockLlm::new(steps),
            terminator_input: Mutex::new(None),
        }
    }

    fn terminator_input(&self) -> Vec<ChatMessage> {
        self.terminator_input
            .lock()
            .expect("terminator input")
            .clone()
            .expect("terminator was invoked")
    }
}

#[async_trait]
impl LlmProvider for QuotingTerminatorLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        *self.terminator_input.lock().expect("terminator input") = Some(messages.to_vec());
        Ok(ChatMessage::assistant(
            serde_json::to_string(messages).expect("conversation json"),
        ))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        self.scripted.complete_with_tools(messages, tools).await
    }
}

/// max tool rounds + ConversationReplay へ到達させる composition。
fn replay_fixture(
    steps: Vec<LlmStepResult>,
    max_rounds: u32,
) -> (RequestService, Arc<QuotingTerminatorLlm>, tempfile::TempDir) {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let llm = Arc::new(QuotingTerminatorLlm::new(steps));
    let log = Arc::new(ToolLog::default());
    let executors: Vec<Arc<dyn ToolExecutor>> = vec![
        Arc::new(RecordingTool {
            name: ToolName::write_file(),
            log: Arc::clone(&log),
        }),
        Arc::new(RecordingTool {
            name: ToolName::read_file(),
            log: Arc::clone(&log),
        }),
    ];
    let registry = Arc::new(DefaultToolRegistry::from_executors(executors).expect("registry"));
    let tools = ToolsConfig {
        max_rounds,
        termination_strategy: TerminationStrategy::ConversationReplay,
        ..Default::default()
    };
    let strategy = tools.termination_strategy;
    let store = Arc::new(ConversationStore::new(dir.path().join("conversations")));
    let profiles = ProfileRegistry::single(
        "default",
        llm.clone(),
        TerminationCapability {
            plain_complete_accepts_tool_role: true,
        },
    );
    let (rpc_extension, turn_hook) = basic_pack_arc();
    let service = RequestService::new(
        profiles,
        registry,
        tools,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".into(),
        store,
        StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    );
    (service, llm, dir)
}

fn request(id: &str, prompt: &str, active: bool, tools: Vec<&str>) -> ClientRequest {
    ClientRequest::AgentTurn {
        id: id.into(),
        messages: vec![ProtocolMessage {
            role: "user".into(),
            content: prompt.into(),
        }],
        tools: tools.into_iter().map(str::to_string).collect(),
        client_tools: vec![],
        context: RequestContext {
            cwd: Some(std::env::current_dir().expect("cwd").display().to_string()),
            task_completion: active,
            ai_session_id: Some(format!("session-{id}")),
            conversation_id: Some(format!("conversation-{id}")),
            ..Default::default()
        },
        llm_profile: None,
    }
}

fn tool_messages(messages: &[aibe::domain::ChatMessage]) -> Vec<(&str, &str)> {
    messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .map(|message| {
            (
                message.tool_call_id.as_deref().expect("tool result id"),
                message.content.as_str(),
            )
        })
        .collect()
}

fn assert_control_definition(definitions: &[ToolDefinition], expected: usize) {
    assert_eq!(
        definitions
            .iter()
            .filter(|definition| definition.name == CONTROL)
            .count(),
        expected
    );
}

#[tokio::test]
async fn task_contract_control_function_vertical_e2e() {
    let fixture = fixture(vec![
        LlmStepResult::with_tool_calls("", vec![control("contract", contract("create artifact"))]),
        LlmStepResult::with_tool_calls(
            "",
            vec![call(
                "write",
                WRITE_FILE,
                json!({"path":"artifact.txt","mode":"create","content":"created\n"}),
            )],
        ),
        LlmStepResult::with_tool_calls(
            "",
            vec![call("read", READ_FILE, json!({"path":"artifact.txt"}))],
        ),
        LlmStepResult::text_only(done(&["e2"])),
    ]);
    let response = fixture
        .service
        .handle(
            request(
                "vertical",
                "create artifact",
                true,
                vec![WRITE_FILE, READ_FILE],
            ),
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult {
        status,
        assistant_message,
        tool_calls,
        completion_report: Some(report),
        ..
    } = response
    else {
        panic!("vertical request did not complete")
    };
    assert_eq!(status, AgentTurnStatus::Ok);
    assert_eq!(report.outcome, CompletionOutcome::Done);
    assert_eq!(report.queries_used, 1);
    assert!(report.criteria[0]
        .evidence
        .iter()
        .any(|item| item.evidence_id == "e2"));
    assert!(!assistant_message.content.contains("\"contract\""));
    assert_eq!(
        fixture.log.0.lock().expect("log").as_slice(),
        &[
            ("write".into(), WRITE_FILE.into()),
            ("read".into(), READ_FILE.into())
        ]
    );
    assert_eq!(
        tool_calls.len(),
        2,
        "control must not be an executed/audit tool"
    );
    let calls = fixture.llm.recorded_calls();
    assert_eq!(
        calls.len(),
        4,
        "no Contract-only query outside the Query Loop"
    );
    assert!(tool_messages(&calls[1])
        .iter()
        .any(|(id, body)| *id == "contract" && *body == r#"{"status":"contract_fixed"}"#));
    for definitions in fixture.llm.recorded_tool_definitions() {
        assert_control_definition(&definitions, 1);
    }
}

#[tokio::test]
async fn control_tool_visibility_is_server_owned() {
    let active = fixture(vec![
        LlmStepResult::with_tool_calls("", vec![control("c", contract("create artifact"))]),
        LlmStepResult::text_only(blocked()),
    ]);
    let _ = active
        .service
        .handle(
            request("active", "create artifact", true, vec![WRITE_FILE]),
            None,
        )
        .await;
    assert_control_definition(&active.llm.recorded_tool_definitions()[0], 1);

    let inactive = fixture(vec![LlmStepResult::text_only("plain")]);
    let _ = inactive
        .service
        .handle(
            request("inactive", "create artifact", false, vec![WRITE_FILE]),
            None,
        )
        .await;
    assert_control_definition(&inactive.llm.recorded_tool_definitions()[0], 0);

    let noneligible = fixture(vec![LlmStepResult::text_only("plain")]);
    let _ = noneligible
        .service
        .handle(request("noneligible", "explain this", true, vec![]), None)
        .await;
    assert!(
        noneligible.llm.recorded_tool_definitions().is_empty(),
        "a turn without tools uses no provider tool-definition call"
    );

    let reserved = fixture(vec![]);
    let mut request = request("reserved", "create artifact", true, vec![WRITE_FILE]);
    let ClientRequest::AgentTurn { client_tools, .. } = &mut request else {
        unreachable!()
    };
    client_tools.push(ClientProvidedToolSpec {
        name: CONTROL.into(),
        description: "attacker override".into(),
        parameters: json!({"type":"object"}),
        risk_class: ToolRiskClass::ReadOnly,
        max_output_bytes: 1024,
    });
    assert!(matches!(
        reserved.service.handle(request, None).await,
        ClientResponse::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(reserved.llm.recorded_tool_definitions().is_empty());
    assert!(!aibe::domain::KNOWN_TOOLS.contains(&CONTROL));
}

#[tokio::test]
async fn control_args_fix_contract_before_effect() {
    let gate = ContractGate::strict(
        TaskCompletionEligibility::Active {
            expected_kind: TaskKind::Execution,
        },
        "create artifact",
    );
    assert!(gate
        .fix_contract_from_control_args(&contract("create artifact"))
        .is_ok());
    assert!(gate.fixed_contract().expect("gate").is_some());
    for invalid in [
        json!({"goal":"create artifact"}),
        json!({"goal":"create artifact","unexpected":true}),
        {
            let mut value = contract("create artifact");
            value["task_kind"] = json!("plan");
            value
        },
    ] {
        let gate = ContractGate::strict(
            TaskCompletionEligibility::Active {
                expected_kind: TaskKind::Execution,
            },
            "create artifact",
        );
        assert!(gate.fix_contract_from_control_args(&invalid).is_err());
        assert!(!gate.is_fixed().expect("gate"));
    }
    let gate = ContractGate::strict(
        TaskCompletionEligibility::Active {
            expected_kind: TaskKind::Execution,
        },
        "create artifact",
    );
    let oversized = oversized_contract();
    assert!(gate.fix_contract_from_control_args(&oversized).is_err());

    let fixture = fixture(vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![
                control("bad", json!({"goal":"create artifact"})),
                call("write", WRITE_FILE, json!({"path":"artifact.txt"})),
            ],
        ),
        LlmStepResult::with_tool_calls("", vec![control("good", contract("create artifact"))]),
        LlmStepResult::text_only(blocked()),
    ]);
    let response = fixture
        .service
        .handle(
            request("args", "create artifact", true, vec![WRITE_FILE]),
            None,
        )
        .await;
    assert!(fixture.log.0.lock().expect("log").is_empty());
    let ClientResponse::AgentTurnResult {
        tool_calls,
        completion_report: Some(report),
        ..
    } = response
    else {
        panic!("closed result")
    };
    assert!(
        tool_calls.is_empty(),
        "control is neither effect nor audit record"
    );
    assert!(report
        .criteria
        .iter()
        .all(|criterion| criterion.evidence.is_empty()));
    let calls = fixture.llm.recorded_calls();
    let first_results = tool_messages(&calls[1]);
    assert_eq!(first_results.len(), 2);
    assert!(first_results[0].1.contains("invalid_task_contract"));
    assert!(first_results[1].1.contains("skipped_before_execution"));
    let definitions = fixture.llm.recorded_tool_definitions();
    let definition = definitions[0]
        .iter()
        .find(|definition| definition.name == CONTROL)
        .expect("server control definition");
    let properties = definition.parameters["properties"]
        .as_object()
        .expect("properties");
    for field in [
        "goal",
        "task_kind",
        "criteria",
        "constraints",
        "deliverables",
        "verification",
        "verification_tools",
        "delegated_verification",
    ] {
        assert!(
            properties.contains_key(field),
            "schema field drift: {field}"
        );
    }
    assert_eq!(properties.len(), 8);

    let delegated = &properties["delegated_verification"];
    assert_eq!(delegated["type"], "object");
    assert_eq!(delegated["additionalProperties"], false);
    assert_eq!(delegated["required"], json!(["items"]));
    let items = &delegated["properties"]["items"];
    assert_eq!(items["minItems"], 1);
    assert_eq!(items["maxItems"], 32);
    let item = &items["items"];
    assert_eq!(item["type"], "object");
    assert_eq!(item["additionalProperties"], false);
    assert_eq!(
        item["required"],
        json!(["id", "criterion_ids", "action", "expected_success"])
    );
    assert_eq!(item["properties"]["id"]["minLength"], 1);
    assert_eq!(item["properties"]["id"]["maxLength"], 8192);
    let criterion_ids = &item["properties"]["criterion_ids"];
    assert_eq!(criterion_ids["minItems"], 1);
    assert_eq!(criterion_ids["maxItems"], 32);
    assert_eq!(criterion_ids["items"]["minLength"], 1);
    assert_eq!(criterion_ids["items"]["maxLength"], 8192);
    assert_eq!(item["properties"]["expected_success"]["minLength"], 1);
    assert_eq!(item["properties"]["expected_success"]["maxLength"], 8192);

    let actions = item["properties"]["action"]["oneOf"]
        .as_array()
        .expect("tagged action variants");
    assert_eq!(actions.len(), 2);
    for action in actions {
        assert_eq!(action["type"], "object");
        assert_eq!(action["additionalProperties"], false);
    }
    assert_eq!(actions[0]["properties"]["kind"]["const"], "observation");
    assert_eq!(actions[0]["required"], json!(["kind", "tool", "target"]));
    assert_eq!(actions[0]["properties"]["tool"]["minLength"], 1);
    assert_eq!(actions[0]["properties"]["tool"]["maxLength"], 8192);
    assert_eq!(actions[0]["properties"]["target"]["minLength"], 1);
    assert_eq!(actions[0]["properties"]["target"]["maxLength"], 8192);
    assert_eq!(actions[1]["properties"]["kind"]["const"], "command");
    assert_eq!(actions[1]["required"], json!(["kind", "command", "cwd"]));
    assert_eq!(actions[1]["properties"]["command"]["minLength"], 1);
    assert_eq!(actions[1]["properties"]["command"]["maxLength"], 8192);
    assert_eq!(actions[1]["properties"]["cwd"]["minLength"], 1);
    assert_eq!(actions[1]["properties"]["cwd"]["maxLength"], 8192);
    assert_eq!(actions[1]["properties"]["args"]["maxItems"], 32);
    assert_eq!(actions[1]["properties"]["args"]["items"]["maxLength"], 8192);
}

#[tokio::test]
async fn mixed_step_control_precedes_effects() {
    for (case, calls) in [
        (
            "control-last",
            vec![
                call("write", WRITE_FILE, json!({"path":"artifact.txt"})),
                call("read", READ_FILE, json!({"path":"artifact.txt"})),
                control("control", contract("create artifact")),
            ],
        ),
        (
            "control-first",
            vec![
                control("control", contract("create artifact")),
                call("write", WRITE_FILE, json!({"path":"artifact.txt"})),
                call("read", READ_FILE, json!({"path":"artifact.txt"})),
            ],
        ),
    ] {
        let fixture = fixture(vec![
            LlmStepResult::with_tool_calls("", calls),
            LlmStepResult::text_only(done(&["e2"])),
        ]);
        let response = fixture
            .service
            .handle(
                request(case, "create artifact", true, vec![WRITE_FILE, READ_FILE]),
                None,
            )
            .await;
        let ClientResponse::AgentTurnResult {
            tool_calls,
            completion_report: Some(report),
            ..
        } = response
        else {
            panic!("mixed step")
        };
        assert_eq!(report.outcome, CompletionOutcome::Done);
        assert_eq!(
            tool_calls
                .iter()
                .map(|call| call.id.as_str())
                .collect::<Vec<_>>(),
            ["write", "read"]
        );
        assert_eq!(
            fixture.log.0.lock().expect("log").as_slice(),
            &[
                ("write".into(), WRITE_FILE.into()),
                ("read".into(), READ_FILE.into())
            ]
        );
        let ids = tool_messages(&fixture.llm.recorded_calls()[1])
            .into_iter()
            .map(|(id, _)| id.to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 3);
        assert!(
            ids.contains(&"control".into())
                && ids.contains(&"write".into())
                && ids.contains(&"read".into())
        );
    }
}

#[tokio::test]
async fn missing_invalid_or_duplicate_control_fails_closed() {
    struct Case {
        name: &'static str,
        prefix: Vec<LlmStepResult>,
        bad: Vec<ToolCall>,
        cause: &'static str,
    }
    let cases = vec![
        Case {
            name: "missing",
            prefix: vec![],
            bad: vec![
                call("write", WRITE_FILE, json!({})),
                call("read", READ_FILE, json!({})),
                call("unknown", "unknown_tool", json!({})),
            ],
            cause: "task_contract_required",
        },
        Case {
            name: "invalid",
            prefix: vec![],
            bad: vec![
                control("bad", json!({"goal":"create artifact"})),
                call("write", WRITE_FILE, json!({})),
            ],
            cause: "invalid_task_contract",
        },
        Case {
            name: "oversized",
            prefix: vec![],
            bad: vec![
                control("oversized", oversized_contract()),
                call("write", WRITE_FILE, json!({})),
            ],
            cause: "invalid_task_contract",
        },
        Case {
            name: "duplicate",
            prefix: vec![],
            bad: vec![
                control("c1", contract("create artifact")),
                control("c2", contract("create artifact")),
                call("write", WRITE_FILE, json!({})),
            ],
            cause: "duplicate_task_contract_control",
        },
        Case {
            name: "already",
            prefix: vec![LlmStepResult::with_tool_calls(
                "",
                vec![control("fixed", contract("create artifact"))],
            )],
            bad: vec![
                control("again", json!({"invalid":true})),
                call("write", WRITE_FILE, json!({})),
            ],
            cause: "task_contract_already_fixed",
        },
        Case {
            name: "duplicate-already-fixed",
            prefix: vec![LlmStepResult::with_tool_calls(
                "",
                vec![control("fixed", contract("create artifact"))],
            )],
            bad: vec![
                control("again-1", json!({"invalid":true})),
                control("again-2", contract("create artifact")),
                call("write", WRITE_FILE, json!({})),
            ],
            cause: "duplicate_task_contract_control",
        },
    ];
    for case in cases {
        let prefix_len = case.prefix.len();
        let expected_ids = case
            .bad
            .iter()
            .map(|call| call.id.clone())
            .collect::<Vec<_>>();
        let mut steps = case.prefix;
        steps.push(LlmStepResult::with_tool_calls("", case.bad));
        if prefix_len == 0 {
            steps.push(LlmStepResult::with_tool_calls(
                "",
                vec![control("recovery", contract("create artifact"))],
            ));
        }
        steps.push(LlmStepResult::text_only(blocked()));
        let fixture = fixture(steps);
        let _ = fixture
            .service
            .handle(
                request(
                    case.name,
                    "create artifact",
                    true,
                    vec![WRITE_FILE, READ_FILE],
                ),
                None,
            )
            .await;
        assert!(
            fixture.log.0.lock().expect("log").is_empty(),
            "{}",
            case.name
        );
        let next_round = &fixture.llm.recorded_calls()[prefix_len + 1];
        let all_results = tool_messages(next_round);
        let results = &all_results[all_results.len() - expected_ids.len()..];
        assert_eq!(
            results.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            expected_ids.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(results.len(), expected_ids.len());
        assert_eq!(
            results
                .iter()
                .filter(|(_, body)| body.contains(case.cause))
                .count(),
            results.len()
        );
        assert_eq!(
            results
                .iter()
                .filter(|(_, body)| body.contains("\"code\":\"skipped_before_execution\""))
                .count(),
            results.len().saturating_sub(1)
        );
    }

    for bad_id in ["", "same"] {
        let calls = if bad_id.is_empty() {
            vec![control("", contract("create artifact"))]
        } else {
            vec![
                control("same", contract("create artifact")),
                call("same", WRITE_FILE, json!({})),
            ]
        };
        let fixture = fixture(vec![LlmStepResult::with_tool_calls("", calls)]);
        assert!(matches!(
            fixture
                .service
                .handle(
                    request(
                        &format!("id-{bad_id}"),
                        "create artifact",
                        true,
                        vec![WRITE_FILE]
                    ),
                    None
                )
                .await,
            ClientResponse::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ));
        assert!(fixture.log.0.lock().expect("log").is_empty());
    }
}

#[tokio::test]
async fn active_text_contract_path_is_rejected() {
    let text = json!({"aish_task_completion":{"contract":contract("create artifact")}}).to_string();
    let text_fixture = fixture(vec![
        LlmStepResult::with_tool_calls(
            &text,
            vec![
                control("control", contract("create artifact")),
                control("duplicate", json!({"invalid":true})),
                call("write", WRITE_FILE, json!({})),
            ],
        ),
        LlmStepResult::with_tool_calls("", vec![control("recovery", contract("create artifact"))]),
        LlmStepResult::text_only(blocked()),
    ]);
    let _ = text_fixture
        .service
        .handle(
            request("text", "create artifact", true, vec![WRITE_FILE]),
            None,
        )
        .await;
    assert!(text_fixture.log.0.lock().expect("log").is_empty());
    let recorded_calls = text_fixture.llm.recorded_calls();
    let results = tool_messages(&recorded_calls[1]);
    assert_eq!(results.len(), 3);
    assert!(results
        .iter()
        .all(|(_, body)| body.contains("text_task_contract_forbidden")));
    assert!(!ContractGate::text_contract_forbidden(
        "documentation mentions aish_task_completion.contract"
    ));

    let inactive = fixture(vec![LlmStepResult::text_only(text.clone())]);
    let response = inactive
        .service
        .handle(
            request("inactive-text", "explain", false, vec![WRITE_FILE]),
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult {
        assistant_message,
        completion_report,
        ..
    } = response
    else {
        panic!("inactive")
    };
    assert_eq!(assistant_message.content, text);
    assert!(completion_report.is_none());
}

#[tokio::test]
async fn text_evaluation_transport_remains_compatible() {
    assert!(decode_completion_envelope(&done(&[]))
        .expect("evaluation")
        .present()
        .is_some());
    assert_eq!(
        decode_completion_envelope("no envelope here").expect("absent"),
        CompletionEnvelopeDecode::Absent
    );

    // evaluation 欠落だけは、他の decode / validation error と別の結果になる。
    // 0072 recovery instruction builder はこの結果だけを入力にできる。
    let missing_evaluation = json!({"aish_task_completion":{},"deliverable":"partial"}).to_string();
    assert_eq!(
        decode_completion_envelope(&missing_evaluation).expect("missing evaluation"),
        CompletionEnvelopeDecode::MissingEvaluation
    );

    // 明示的な null は field 欠落ではなく shape 不正。MissingEvaluation へ緩和しない。
    let null_evaluation =
        json!({"aish_task_completion":{"evaluation":null},"deliverable":"partial"}).to_string();
    assert!(decode_completion_envelope(&null_evaluation).is_err());
    let wrong_type_evaluation =
        json!({"aish_task_completion":{"evaluation":"done"},"deliverable":"partial"}).to_string();
    assert!(decode_completion_envelope(&wrong_type_evaluation).is_err());

    let legacy = json!({"aish_task_completion":{"contract":contract("create artifact"),"evaluation":{"criteria":[]}},"deliverable":"done"}).to_string();
    assert!(decode_completion_envelope(&legacy).is_err());
    let unknown = json!({"aish_task_completion":{"evaluation":{"criteria":[]},"unknown":true},"deliverable":"done"}).to_string();
    assert!(decode_completion_envelope(&unknown).is_err());
    let malformed = "{\"aish_task_completion\":oops}";
    assert!(decode_completion_envelope(malformed).is_err());
    // Contract member を持つ legacy shape は evaluation 欠落ではなく error 側に残る。
    let legacy_without_evaluation =
        json!({"aish_task_completion":{"contract":contract("create artifact")},"deliverable":"done"})
            .to_string();
    assert!(decode_completion_envelope(&legacy_without_evaluation).is_err());
    // bound 違反も evaluation 欠落と混同しない。
    assert!(decode_completion_envelope(&format!(
        "{{\"aish_task_completion\":{{}},\"deliverable\":\"{}\"}}",
        "x".repeat(ENVELOPE_MAX_BYTES)
    ))
    .is_err());
    assert!(decode_completion_envelope(
        &json!({"aish_task_completion":{},"deliverable":"x".repeat(CONTRACT_TEXT_MAX_BYTES + 1)})
            .to_string()
    )
    .is_err());

    let evaluation_fixture = fixture(vec![
        LlmStepResult::with_tool_calls("", vec![control("c", contract("create artifact"))]),
        LlmStepResult::text_only(blocked()),
    ]);
    assert!(matches!(
        evaluation_fixture
            .service
            .handle(
                request("evaluation", "create artifact", true, vec![WRITE_FILE]),
                None
            )
            .await,
        ClientResponse::AgentTurnResult {
            completion_report: Some(_),
            ..
        }
    ));

    let legacy_fixture = fixture(vec![
        LlmStepResult::with_tool_calls("", vec![control("c", contract("create artifact"))]),
        LlmStepResult::text_only(legacy),
    ]);
    assert!(matches!(
        legacy_fixture
            .service
            .handle(
                request("legacy", "create artifact", true, vec![WRITE_FILE]),
                None
            )
            .await,
        ClientResponse::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
}

#[tokio::test]
async fn provider_function_call_args_are_portable() {
    let contract = contract("create artifact");
    let gemini_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{
                "thoughtSignature": "sig",
                "functionCall": {
                    "name": CONTROL,
                    "args": contract.clone()
                }
            }]}}]
        })))
        .mount(&gemini_server)
        .await;
    let gemini = GeminiLlm::new(
        format!("{}/v1beta", gemini_server.uri()),
        "test-key".into(),
        "test-model".into(),
    );
    let gemini_step = gemini
        .complete_with_tools(
            &[ChatMessage::user("create artifact")],
            &[aibe::application::tool_defs::task_contract_control_definition()],
        )
        .await
        .expect("Gemini functionCall response");
    assert!(gemini_step.assistant.content.is_empty());
    assert_eq!(gemini_step.tool_calls[0].id, "call_0_0");
    assert_eq!(
        gemini_step.tool_calls[0].provider_extras,
        Some(json!({"gemini_part_index":0,"thoughtSignature":"sig"}))
    );

    let openai_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "provider-call",
                    "type": "function",
                    "function": {
                        "name": CONTROL,
                        "arguments": contract.to_string()
                    }
                }]
            }}]
        })))
        .mount(&openai_server)
        .await;
    let openai = OpenAiCompatibleLlm::new(
        format!("{}/v1", openai_server.uri()),
        "test-key".into(),
        "test-model".into(),
    );
    let openai_step = openai
        .complete_with_tools(
            &[ChatMessage::user("create artifact")],
            &[aibe::application::tool_defs::task_contract_control_definition()],
        )
        .await
        .expect("OpenAI-compatible tool_calls response");
    assert!(openai_step.assistant.content.is_empty());
    assert_eq!(openai_step.tool_calls[0].id, "provider-call");
    assert_eq!(openai_step.tool_calls[0].provider_extras, None);

    for (family, step) in [("gemini", gemini_step), ("openai", openai_step)] {
        assert_eq!(step.tool_calls.len(), 1);
        let portable = &step.tool_calls[0];
        assert_eq!(portable.name, CONTROL);
        assert_eq!(portable.arguments, contract);
        let gate = ContractGate::strict(
            TaskCompletionEligibility::Active {
                expected_kind: TaskKind::Execution,
            },
            "create artifact",
        );
        gate.fix_contract_from_control_args(&portable.arguments)
            .expect("adapter arguments fix the contract");
        assert_eq!(
            serde_json::to_value(
                gate.fixed_contract()
                    .expect("gate")
                    .expect("fixed contract")
            )
            .expect("fixed contract JSON"),
            contract
        );

        let fixture = fixture(vec![step, LlmStepResult::text_only(blocked())]);
        let _ = fixture
            .service
            .handle(
                request(family, "create artifact", true, vec![WRITE_FILE]),
                None,
            )
            .await;
        let calls = fixture.llm.recorded_calls();
        assert_eq!(calls[0].last().expect("user").content, "create artifact");
        assert!(tool_messages(&calls[1])
            .iter()
            .any(|(_, body)| *body == r#"{"status":"contract_fixed"}"#));
    }
}

#[tokio::test]
async fn control_security_and_task_completion_regressions_are_preserved() {
    let secret = "0073-SECRET-control-args";
    let prompt = "create artifact";
    let mut secret_contract = contract(prompt);
    secret_contract["constraints"] = json!([secret]);
    let fixture = fixture(vec![
        LlmStepResult::with_tool_calls("", vec![control("secret-control", secret_contract)]),
        LlmStepResult::with_tool_calls(
            "",
            vec![call(
                "write",
                WRITE_FILE,
                json!({"path":"artifact.txt","content":"created"}),
            )],
        ),
        LlmStepResult::with_tool_calls(
            "",
            vec![call("read", READ_FILE, json!({"path":"artifact.txt"}))],
        ),
        LlmStepResult::text_only(done(&["e2"])),
    ]);
    let response = fixture
        .service
        .handle(
            request("security", prompt, true, vec![WRITE_FILE, READ_FILE]),
            None,
        )
        .await;
    let calls = fixture.llm.recorded_calls();
    let forwarded = calls[1]
        .iter()
        .flat_map(|message| message.tool_calls.iter().flatten())
        .find(|call| call.id == "secret-control")
        .expect("request-local assistant call");
    assert_eq!(forwarded.arguments["constraints"], json!([secret]));
    assert!(!tool_messages(&calls[1])
        .iter()
        .any(|(_, body)| body.contains(secret)));
    let serialized = serde_json::to_string(&response).expect("response json");
    assert!(!serialized.contains(secret));
    let ClientResponse::AgentTurnResult {
        tool_calls,
        completion_report: Some(report),
        ..
    } = &response
    else {
        panic!("Done response: {response:?}")
    };
    assert_eq!(report.outcome, CompletionOutcome::Done);
    assert_eq!(report.queries_used, 1);
    assert_eq!(tool_calls.len(), 2);
    assert!(tool_calls.iter().all(|call| call.name != CONTROL));
    assert!(report.criteria[0]
        .evidence
        .iter()
        .all(|item| !format!("{item:?}").contains(secret)));
    let snapshot = fixture
        .store
        .load_snapshot("session-security", "conversation-security")
        .expect("snapshot read")
        .expect("snapshot");
    assert!(snapshot
        .messages
        .iter()
        .all(|message| !message.content.contains(secret)));

    // MaxToolRounds + ConversationReplay: terminator 入力は client 応答へ引用され得るため、
    // provider wire sink ではなく redact 必須 sink として扱う。
    let mut replay_contract = contract(prompt);
    replay_contract["constraints"] = json!([secret]);
    let mut replay_control = control("secret-control", replay_contract);
    replay_control.provider_extras = Some(json!({"thoughtSignature": secret}));
    let (replay_service, replay_llm, _replay_dir) = replay_fixture(
        vec![
            LlmStepResult::with_tool_calls("", vec![replay_control]),
            LlmStepResult::with_tool_calls(
                "",
                vec![call(
                    "write",
                    WRITE_FILE,
                    json!({"path":"artifact.txt","content":"created"}),
                )],
            ),
        ],
        2,
    );
    let replay_response = replay_service
        .handle(
            request("replay", prompt, true, vec![WRITE_FILE, READ_FILE]),
            None,
        )
        .await;
    let terminator_input = replay_llm.terminator_input();
    let control_args = terminator_input
        .iter()
        .flat_map(|message| message.tool_calls.iter().flatten())
        .filter(|call| call.name == CONTROL)
        .collect::<Vec<_>>();
    assert_eq!(
        control_args.len(),
        1,
        "replay input must still correlate the control call"
    );
    assert_eq!(control_args[0].arguments, json!({"redacted": true}));
    assert!(
        control_args[0].provider_extras.is_none(),
        "provider 固有 part は sentinel ではなく省略する"
    );
    assert!(
        !serde_json::to_string(&terminator_input)
            .expect("terminator input json")
            .contains(secret),
        "raw control args must not reach the terminator input"
    );
    assert!(
        !serde_json::to_string(&replay_response)
            .expect("replay response json")
            .contains(secret),
        "terminator output must not surface raw control args to the client"
    );
}
