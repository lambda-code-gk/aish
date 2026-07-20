//! リクエスト種別のディスパッチ。

use async_trait::async_trait;
use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::application::completion_envelope::{decode_completion_envelope, ContractGate};
use crate::application::route_turn::RouteTurnService;
use crate::application::task_completion::{
    append_evidence_from_tools, build_continuation, build_report, deliverable_evidence,
    evidence_from_tools, system_instruction, CompletionEventBuffer,
};
use crate::application::tool_round::ToolRoundExecutor;
use crate::domain::{
    classify_task_completion_eligibility, is_stalled, parse_tool_names, progress_snapshot,
    validate_contract_covers_request, AgentTurnContext, ChatMessage, ClientCwd, ClientCwdError,
    CompletionEvaluation, EvidenceRecord, FeatureEligibilityContext, FeatureRegistry,
    TaskCompletionEligibility, TaskContract,
};
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::outbound::{
    CapabilityPolicy, ClientToolGate, ConversationStore, HumanTaskGate, LlmCallTracer,
    ProfileRegistry, RouterConfig, RpcExtension, ShellExecApprovalGate, ToolApprovalGate,
    ToolRoundTerminator, ToolsConfig, TurnCancellation, TurnEventSink, TurnHook,
};
use aibe_protocol::{
    ClientProvidedToolSpec, ClientRequest, ClientResponse, CompletionReport, ErrorCode,
    MemorySubscribeRequestBody, ProtocolMessage, RequestContext, ToolRiskClass,
};
use std::collections::{HashMap, HashSet};
use tokio::sync::Mutex;

use crate::application::protocol_convert::ProtocolMessageConversionError;

pub struct RequestService {
    profile_registry: ProfileRegistry,
    tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
    tools_config: ToolsConfig,
    terminator: Arc<dyn ToolRoundTerminator>,
    active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
    completed_continuation_turns: Arc<Mutex<HashSet<String>>>,
    router_profile: String,
    conversation_store: Arc<dyn ConversationStore>,
    capability_policy: Arc<dyn CapabilityPolicy>,
    rpc_extension: Arc<dyn RpcExtension>,
    turn_hook: Arc<dyn TurnHook>,
    feature_registry: FeatureRegistry,
    feature_eligibility: FeatureEligibilityContext,
    llm_tracer: Arc<dyn LlmCallTracer>,
}

impl RequestService {
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_turns_and_packs(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        rpc_extension: Arc<dyn RpcExtension>,
        turn_hook: Arc<dyn TurnHook>,
        feature_registry: FeatureRegistry,
        feature_eligibility: FeatureEligibilityContext,
        llm_tracer: Arc<dyn LlmCallTracer>,
    ) -> Self {
        Self {
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
            completed_continuation_turns: Arc::new(Mutex::new(HashSet::new())),
            router_profile,
            conversation_store,
            capability_policy,
            rpc_extension,
            turn_hook,
            feature_registry,
            feature_eligibility,
            llm_tracer,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        rpc_extension: Arc<dyn RpcExtension>,
        turn_hook: Arc<dyn TurnHook>,
        feature_registry: FeatureRegistry,
    ) -> Self {
        Self::new_with_turns_and_packs(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            Arc::new(Mutex::new(HashMap::new())),
            router_profile,
            conversation_store,
            capability_policy,
            rpc_extension,
            turn_hook,
            feature_registry,
            FeatureEligibilityContext::default(),
            Arc::new(crate::ports::outbound::NoopLlmCallTracer),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_turns(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        rpc_extension: Arc<dyn RpcExtension>,
        turn_hook: Arc<dyn TurnHook>,
        feature_registry: FeatureRegistry,
    ) -> Self {
        Self::new_with_turns_and_packs(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
            router_profile,
            conversation_store,
            capability_policy,
            rpc_extension,
            turn_hook,
            feature_registry,
            FeatureEligibilityContext::default(),
            Arc::new(crate::ports::outbound::NoopLlmCallTracer),
        )
    }

    pub async fn handle(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
    ) -> ClientResponse {
        self.handle_with_client_tool_gate(request, approval_gate, None)
            .await
    }

    pub async fn handle_with_client_tool_gate(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
    ) -> ClientResponse {
        self.handle_with_events(
            request,
            approval_gate,
            None,
            client_tool_gate,
            None,
            None,
            None,
        )
        .await
    }

    pub async fn cancel_all_active_turns(&self) {
        let guard = self.active_turns.lock().await;
        for cancel in guard.values() {
            cancel.cancel();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        tool_approval_gate: Option<Arc<dyn ToolApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
        human_task_gate: Option<Arc<dyn HumanTaskGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse {
        match request {
            ClientRequest::Ping { id } => ClientResponse::Pong { id },
            ClientRequest::RouteTurn {
                id,
                query,
                cwd,
                session,
                conversation,
                cli_overrides,
            } => {
                let route_service = RouteTurnService::new(
                    self.profile_registry.clone(),
                    RouterConfig {
                        profile: self.router_profile.clone(),
                    },
                    self.conversation_store.clone(),
                    self.feature_registry.clone(),
                    self.feature_eligibility,
                    Arc::clone(&self.llm_tracer),
                );
                route_service
                    .run(id, query, cwd, session, conversation, cli_overrides)
                    .await
            }
            ClientRequest::CancelTurn { id, turn_id } => {
                let guard = self.active_turns.lock().await;
                if let Some(cancel) = guard.get(&turn_id) {
                    cancel.cancel();
                    ClientResponse::Cancelled {
                        id,
                        turn_id,
                        reason: Some("cancel requested".into()),
                    }
                } else {
                    ClientResponse::Cancelled {
                        id,
                        turn_id,
                        reason: Some("turn not active".into()),
                    }
                }
            }
            ClientRequest::ShellExecApproval { .. } => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "shell_exec_approval must be sent during an active agent_turn",
            ),
            ClientRequest::ToolApproval { .. } => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "tool_approval must be sent during an active agent_turn",
            ),
            ClientRequest::ClientToolResult(_) => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "client_tool_result must be sent during an active agent_turn",
            ),
            ClientRequest::HumanTaskExecutionResult { .. } => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "human_task_execution_result must be sent during an active agent_turn",
            ),
            ClientRequest::MemoryApply(body) => self.rpc_extension.memory_apply(body),
            ClientRequest::MemoryQuery(body) => self.rpc_extension.memory_query(body),
            ClientRequest::MemoryKindList(body) => self.rpc_extension.memory_kind_list(body),
            ClientRequest::MemoryRecipeRun(body) => {
                self.rpc_extension.memory_recipe_run(body).await
            }
            ClientRequest::WorkApply(body) => self.rpc_extension.work_apply(body),
            ClientRequest::WorkQuery(body) => self.rpc_extension.work_query(body),
            ClientRequest::MemorySubscribe(_) => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "memory_subscribe must use a dedicated connection handler",
            ),
            ClientRequest::AgentTurn {
                id,
                messages,
                tools,
                client_tools,
                context,
                llm_profile,
            } => {
                let continuation_turn = context.continuation_turn;
                let (llm, termination_capability) =
                    match self.profile_registry.resolve(llm_profile.as_deref()) {
                        Ok(pair) => pair,
                        Err(msg) => {
                            return ClientResponse::error(id, ErrorCode::InvalidRequest, msg);
                        }
                    };

                let messages: Vec<ChatMessage> = match messages
                    .into_iter()
                    .map(ChatMessage::try_from)
                    .collect::<Result<Vec<_>, ProtocolMessageConversionError>>()
                {
                    Ok(msgs) => msgs,
                    Err(e) => {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
                    }
                };

                if !tools.is_empty() {
                    if let Err(e) = validate_client_cwd_for_tools(&context) {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
                    }
                }

                let _request_messages = messages.clone();
                let conversation_id = context.conversation_id.clone();
                let ai_session_id = context.ai_session_id.clone();
                let tools = match parse_tool_names(tools) {
                    Ok(names) => names,
                    Err(e) => {
                        return ClientResponse::error(id, ErrorCode::ToolNotAllowed, e.to_string());
                    }
                };
                let eligibility = classify_task_completion_eligibility(
                    context.task_completion,
                    &tools.iter().map(|name| name.as_str()).collect::<Vec<_>>(),
                );
                let client_tools = match validate_client_tools(client_tools) {
                    Ok(tools) => tools,
                    Err(message) => {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, message);
                    }
                };

                let ctx = match AgentTurnContext::try_from(context) {
                    Ok(ctx) => ctx,
                    Err(e) => match e {},
                };

                let turn_id = id.clone();
                let effective_cancellation = cancellation
                    .clone()
                    .unwrap_or_else(|| Arc::new(TurnCancellation::new()));
                {
                    let mut active = self.active_turns.lock().await;
                    let completed = self.completed_continuation_turns.lock().await;
                    if active.contains_key(&turn_id)
                        || (continuation_turn && completed.contains(&turn_id))
                    {
                        return ClientResponse::error(
                            id,
                            ErrorCode::InvalidRequest,
                            "duplicate agent turn id in this aibe process",
                        );
                    }
                    active.insert(turn_id.clone(), Arc::clone(&effective_cancellation));
                }

                let mut run_messages = messages.clone();
                if let (Some(session_id), Some(conv_id)) = (&ai_session_id, &conversation_id) {
                    if let Ok(Some(snapshot)) =
                        self.conversation_store.load_snapshot(session_id, conv_id)
                    {
                        if !snapshot.messages.is_empty() {
                            let stored: Result<Vec<ChatMessage>, _> = snapshot
                                .messages
                                .into_iter()
                                .map(ChatMessage::try_from)
                                .collect();
                            if let Ok(stored) = stored {
                                run_messages = stored;
                                run_messages.extend(messages);
                            }
                        }
                    }
                }

                // Task Completion の内部制御 instruction は対象 turn にだけ挿入する。
                // ConversationStore へ保存すると、継続 turn ごとに内部 prompt が履歴へ累積する。
                let conversation_record_messages = run_messages.clone();
                let user_request = last_user_request(&conversation_record_messages);
                let contract_gate = Arc::new(match eligibility {
                    TaskCompletionEligibility::Active { .. } => {
                        ContractGate::strict(eligibility, user_request.clone())
                    }
                    TaskCompletionEligibility::Inactive => ContractGate::permissive(),
                });
                let executor = ToolRoundExecutor::new(
                    Arc::clone(llm),
                    Arc::clone(&self.tool_registry),
                    self.tools_config.clone(),
                    Arc::clone(&self.llm_tracer),
                )
                .with_contract_gate(Arc::clone(&contract_gate));
                let agent_turn = AgentTurnService::with_capability_policy(
                    Arc::clone(llm),
                    executor,
                    Arc::clone(&self.terminator),
                    termination_capability,
                    Arc::clone(&self.capability_policy),
                    Arc::clone(&self.turn_hook),
                    Arc::clone(&self.llm_tracer),
                );
                if let TaskCompletionEligibility::Active { expected_kind } = eligibility {
                    run_messages.insert(0, ChatMessage::system(system_instruction(expected_kind)));
                }

                let (completion_event_buffer, buffered_events) =
                    completion_event_sink(eligibility, events.clone());

                let mut response = agent_turn
                    .run_with_client_tools_and_events(
                        id.clone(),
                        run_messages.clone(),
                        tools.clone(),
                        client_tools.clone(),
                        ctx.clone(),
                        approval_gate.clone(),
                        tool_approval_gate.clone(),
                        client_tool_gate.clone(),
                        human_task_gate.clone(),
                        buffered_events.clone(),
                        Some(Arc::clone(&effective_cancellation)),
                    )
                    .await;

                let first_payload = match &response {
                    ClientResponse::AgentTurnResult {
                        status: aibe_protocol::AgentTurnStatus::Suspended,
                        ..
                    } => None,
                    ClientResponse::AgentTurnResult {
                        assistant_message,
                        tool_calls,
                        ..
                    } => Some((assistant_message.content.clone(), tool_calls.clone())),
                    _ => None,
                };
                if let Some((content, mut cumulative_calls)) = first_payload {
                    if matches!(eligibility, TaskCompletionEligibility::Active { .. }) {
                        let parsed = decode_completion_envelope(&content);
                        let fixed = contract_gate.fixed_contract();
                        match (parsed, fixed) {
                            (Err(message), _) | (_, Err(message)) => {
                                response = ClientResponse::error(
                                    id.clone(),
                                    ErrorCode::InvalidRequest,
                                    message,
                                );
                            }
                            (Ok(None), Ok(Some(fixed))) => {
                                response = ClientResponse::error(
                                    id.clone(),
                                    ErrorCode::InvalidRequest,
                                    "task completion final envelope lacks evaluation",
                                );
                                let _ = fixed;
                            }
                            (Ok(Some(first_envelope)), Ok(_)) => {
                                if let Err(message) = validate_contract_covers_request(
                                    &first_envelope.aish_task_completion.contract,
                                    eligibility,
                                    &user_request,
                                ) {
                                    response = ClientResponse::error(
                                        id.clone(),
                                        ErrorCode::InvalidRequest,
                                        message,
                                    );
                                } else if let Err(message) =
                                    contract_gate.inspect_before_tools(&content, false)
                                {
                                    response = ClientResponse::error(
                                        id.clone(),
                                        ErrorCode::InvalidRequest,
                                        message,
                                    );
                                } else {
                                    let contract = first_envelope.aish_task_completion.contract;
                                    let evaluation = first_envelope.aish_task_completion.evaluation;
                                    if let Some(evaluation) = evaluation {
                                        let mut evidence =
                                            evidence_from_tools(&contract, &cumulative_calls);
                                        evidence.extend(deliverable_evidence(
                                            &contract,
                                            &first_envelope.deliverable,
                                            evidence.len() + 1,
                                        ));
                                        match build_report(&contract, &evidence, &evaluation, 1, false) {
                                        Ok(report) => attach_completion_report(
                                            &mut response,
                                            first_envelope.deliverable,
                                            cumulative_calls,
                                            report,
                                        ),
                                        Err(message)
                                            if message
                                                == "completion evaluation requires another query" =>
                                        {
                                            let continuation = build_continuation(
                                                &contract,
                                                &evaluation,
                                                &evidence,
                                            );
                                            let mut second_messages =
                                                vec![ChatMessage::user(continuation)];
                                            if let TaskCompletionEligibility::Active {
                                                expected_kind,
                                            } = eligibility
                                            {
                                                second_messages.insert(
                                                    0,
                                                    ChatMessage::system(system_instruction(
                                                        expected_kind,
                                                    )),
                                                );
                                            }
                                            let second = agent_turn
                                                .run_with_client_tools_and_events(
                                                    id.clone(),
                                                    second_messages,
                                                    tools.clone(),
                                                    client_tools.clone(),
                                                    ctx.clone(),
                                                    approval_gate.clone(),
                                                    tool_approval_gate.clone(),
                                                    client_tool_gate.clone(),
                                                    human_task_gate.clone(),
                                                    buffered_events.clone(),
                                                    Some(Arc::clone(&effective_cancellation)),
                                                )
                                                .await;
                                            response = finish_second_query(
                                                id.clone(),
                                                second,
                                                &contract_gate,
                                                &contract,
                                                &evaluation,
                                                &evidence,
                                                &mut cumulative_calls,
                                            );
                                        }
                                        Err(message) => {
                                            response = ClientResponse::error(
                                                id.clone(),
                                                ErrorCode::InvalidRequest,
                                                message,
                                            );
                                        }
                                    }
                                    } else {
                                        response = ClientResponse::error(
                                            id.clone(),
                                            ErrorCode::InvalidRequest,
                                            "task completion final envelope lacks evaluation",
                                        );
                                    }
                                }
                            }
                            (Ok(None), Ok(None)) => {
                                response = ClientResponse::error(
                                    id.clone(),
                                    ErrorCode::InvalidRequest,
                                    "task completion contract required for this request",
                                );
                            }
                        }
                    }
                }
                if let Some(buffer) = completion_event_buffer {
                    buffer.flush_for_response(&id, &response).await;
                }
                if let (Some(conversation_id), Some(ai_session_id)) =
                    (conversation_id, ai_session_id)
                {
                    if let ClientResponse::AgentTurnResult {
                        assistant_message, ..
                    } = &response
                    {
                        let wire_messages: Vec<ProtocolMessage> = conversation_record_messages
                            .iter()
                            .map(|m| ProtocolMessage {
                                role: m.role.to_string(),
                                content: m.content.clone(),
                            })
                            .collect();
                        let _ = self.conversation_store.record_turn(
                            &ai_session_id,
                            &conversation_id,
                            current_time_ms(),
                            &wire_messages,
                            assistant_message,
                            None,
                        );
                    }
                }
                // Continuation duplicate policy (0065): reject while in-flight or after
                // AgentTurnStatus::Ok. MaxToolRounds / Error must not enter this set so
                // ResultPending retry can reuse the same turn ID in this aibe process.
                if continuation_turn
                    && matches!(
                        response,
                        ClientResponse::AgentTurnResult {
                            status: aibe_protocol::AgentTurnStatus::Ok,
                            ..
                        }
                    )
                {
                    self.completed_continuation_turns
                        .lock()
                        .await
                        .insert(turn_id.clone());
                }
                let mut guard = self.active_turns.lock().await;
                let _ = guard.remove(&turn_id);
                response
            }
        }
    }

    pub async fn handle_memory_subscribe(
        &self,
        body: MemorySubscribeRequestBody,
        writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
        lines: Arc<Mutex<crate::ports::inbound::SubscribeConnectionLines>>,
        shutdown: Option<Arc<crate::ports::inbound::ShutdownCoordinator>>,
    ) -> anyhow::Result<()> {
        use crate::application::memory_subscribe_transport::{
            push_memory_subscription_until_disconnect, write_subscribe_response_line,
        };

        let (response, subscription) = self.rpc_extension.memory_subscribe_begin(body.clone());
        write_subscribe_response_line(&writer, &response).await?;
        let Some(subscription) = subscription else {
            return Ok(());
        };
        let memory_space_id = match &response {
            ClientResponse::MemorySubscribeResult {
                memory_space_id, ..
            } => memory_space_id.clone(),
            _ => return Ok(()),
        };
        push_memory_subscription_until_disconnect(
            body.id,
            memory_space_id,
            subscription,
            writer,
            lines,
            shutdown,
        )
        .await
    }
}

fn attach_completion_report(
    response: &mut ClientResponse,
    deliverable: String,
    tool_calls: Vec<aibe_protocol::ExecutedToolCall>,
    report: CompletionReport,
) {
    if let ClientResponse::AgentTurnResult {
        assistant_message,
        tool_calls: response_calls,
        completion_report,
        ..
    } = response
    {
        assistant_message.content = deliverable;
        *response_calls = tool_calls;
        *completion_report = Some(report);
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_second_query(
    id: String,
    mut response: ClientResponse,
    contract_gate: &ContractGate,
    contract: &TaskContract,
    previous_evaluation: &CompletionEvaluation,
    previous_evidence: &[EvidenceRecord],
    cumulative_calls: &mut Vec<aibe_protocol::ExecutedToolCall>,
) -> ClientResponse {
    let (content, second_calls) = match &response {
        ClientResponse::AgentTurnResult {
            status: aibe_protocol::AgentTurnStatus::Suspended,
            ..
        } => return response,
        ClientResponse::AgentTurnResult {
            assistant_message,
            tool_calls,
            ..
        } => (assistant_message.content.clone(), tool_calls.clone()),
        _ => return response,
    };
    let envelope = match decode_completion_envelope(&content) {
        Ok(Some(envelope)) => envelope,
        Ok(None) => {
            return ClientResponse::error(
                id,
                ErrorCode::InvalidRequest,
                "task completion second query lacks envelope",
            )
        }
        Err(message) => return ClientResponse::error(id, ErrorCode::InvalidRequest, message),
    };
    if let Err(message) = contract_gate.inspect_before_tools(&content, false) {
        return ClientResponse::error(id, ErrorCode::InvalidRequest, message);
    }
    if &envelope.aish_task_completion.contract != contract {
        return ClientResponse::error(
            id,
            ErrorCode::InvalidRequest,
            "task contract changed in second query",
        );
    }
    let Some(evaluation) = envelope.aish_task_completion.evaluation else {
        return ClientResponse::error(
            id,
            ErrorCode::InvalidRequest,
            "task completion second query lacks evaluation",
        );
    };
    let mut evidence = append_evidence_from_tools(contract, previous_evidence, &second_calls);
    cumulative_calls.extend(second_calls);
    evidence.extend(deliverable_evidence(
        contract,
        &envelope.deliverable,
        evidence.len() + 1,
    ));
    let stalled = is_stalled(
        &progress_snapshot(previous_evaluation, previous_evidence),
        &progress_snapshot(&evaluation, &evidence),
    );
    match build_report(contract, &evidence, &evaluation, 2, stalled) {
        Ok(report) => {
            attach_completion_report(
                &mut response,
                envelope.deliverable,
                cumulative_calls.clone(),
                report,
            );
            response
        }
        Err(message) => ClientResponse::error(id, ErrorCode::InvalidRequest, message),
    }
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn last_user_request(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.role == crate::domain::MessageRole::User)
        .map(|message| message.content.clone())
        .unwrap_or_default()
}

fn completion_event_sink(
    eligibility: TaskCompletionEligibility,
    events: Option<Arc<dyn TurnEventSink>>,
) -> (
    Option<Arc<CompletionEventBuffer>>,
    Option<Arc<dyn TurnEventSink>>,
) {
    match eligibility {
        TaskCompletionEligibility::Active { .. } => {
            let buffer = events.map(CompletionEventBuffer::new);
            let sink = buffer
                .as_ref()
                .map(|buffer| Arc::clone(buffer) as Arc<dyn TurnEventSink>);
            (buffer, sink)
        }
        TaskCompletionEligibility::Inactive => (None, events),
    }
}

fn validate_client_cwd_for_tools(context: &RequestContext) -> Result<(), ClientCwdError> {
    match context.cwd.as_deref() {
        Some(raw) => ClientCwd::parse(raw).map(|_| ()),
        None => Err(ClientCwdError::Missing),
    }
}

fn validate_client_tools(
    client_tools: Vec<ClientProvidedToolSpec>,
) -> Result<Vec<ClientProvidedToolSpec>, String> {
    let mut out = Vec::new();
    for spec in client_tools {
        if !spec.name.starts_with("aish.") {
            return Err(format!(
                "client tool must use aish. namespace: {}",
                spec.name
            ));
        }
        if spec.risk_class != ToolRiskClass::ReadOnly {
            return Err(format!("client tool must be read_only: {}", spec.name));
        }
        if spec.name != "aish.replay_show" {
            return Err(format!("unsupported client tool: {}", spec.name));
        }
        if spec.max_output_bytes == 0 {
            return Err(format!(
                "client tool max_output_bytes must be > 0: {}",
                spec.name
            ));
        }
        let mut spec = spec;
        spec.max_output_bytes = spec
            .max_output_bytes
            .min(aibe_protocol::MAX_TOOL_OUTPUT_BYTES as u32);
        out.push(spec);
    }
    Ok(out)
}

#[async_trait]
impl ClientRequestHandler for RequestService {
    async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        tool_approval_gate: Option<Arc<dyn ToolApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
        human_task_gate: Option<Arc<dyn HumanTaskGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse {
        RequestService::handle_with_events(
            self,
            request,
            approval_gate,
            tool_approval_gate,
            client_tool_gate,
            human_task_gate,
            events,
            cancellation,
        )
        .await
    }

    async fn handle_memory_subscribe(
        &self,
        body: aibe_protocol::MemorySubscribeRequestBody,
        writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
        lines: Arc<Mutex<crate::ports::inbound::SubscribeConnectionLines>>,
        shutdown: Option<Arc<crate::ports::inbound::ShutdownCoordinator>>,
    ) -> anyhow::Result<()> {
        RequestService::handle_memory_subscribe(self, body, writer, lines, shutdown).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TaskKind;
    use aibe_protocol::{ClientProvidedToolSpec, ToolRiskClass};

    #[derive(Default)]
    struct RecordingSink {
        deltas: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl TurnEventSink for RecordingSink {
        async fn progress(
            &self,
            _id: &str,
            _phase: aibe_protocol::ProgressPhase,
            _message: Option<String>,
        ) {
        }

        async fn assistant_streaming(&self, _id: &str, delta: String) {
            self.deltas.lock().expect("lock").push(delta);
        }

        async fn final_response(&self, _id: &str) {}
    }

    #[tokio::test]
    async fn completion_streaming_buffer_is_active_only() {
        let inactive_sink = Arc::new(RecordingSink::default());
        let (buffer, selected) = completion_event_sink(
            TaskCompletionEligibility::Inactive,
            Some(Arc::clone(&inactive_sink) as Arc<dyn TurnEventSink>),
        );
        assert!(buffer.is_none());
        selected
            .expect("inactive direct sink")
            .assistant_streaming("turn", "direct".into())
            .await;
        assert_eq!(
            inactive_sink.deltas.lock().expect("lock").as_slice(),
            &["direct"]
        );

        let active_sink = Arc::new(RecordingSink::default());
        let (buffer, selected) = completion_event_sink(
            TaskCompletionEligibility::Active {
                expected_kind: TaskKind::Execution,
            },
            Some(Arc::clone(&active_sink) as Arc<dyn TurnEventSink>),
        );
        let buffer = buffer.expect("active buffer");
        selected
            .expect("active buffered sink")
            .assistant_streaming("turn", "buffered".into())
            .await;
        assert!(active_sink.deltas.lock().expect("lock").is_empty());
        buffer
            .flush_for_response(
                "turn",
                &ClientResponse::error("turn".into(), ErrorCode::InvalidRequest, "fail closed"),
            )
            .await;
        assert!(active_sink.deltas.lock().expect("lock").is_empty());
    }

    #[test]
    fn validate_client_tools_accepts_read_only_aish_namespace() {
        let tools = validate_client_tools(vec![ClientProvidedToolSpec {
            name: "aish.replay_show".into(),
            description: "show".into(),
            parameters: serde_json::json!({"type":"object"}),
            risk_class: ToolRiskClass::ReadOnly,
            max_output_bytes: 8192,
        }])
        .expect("valid");
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn validate_client_tools_rejects_non_namespace_and_dangerous_names() {
        for spec in [
            ClientProvidedToolSpec {
                name: "replay_show".into(),
                description: "show".into(),
                parameters: serde_json::json!({}),
                risk_class: ToolRiskClass::ReadOnly,
                max_output_bytes: 8192,
            },
            ClientProvidedToolSpec {
                name: "aish.shell_exec".into(),
                description: "shell".into(),
                parameters: serde_json::json!({}),
                risk_class: ToolRiskClass::ReadOnly,
                max_output_bytes: 8192,
            },
            ClientProvidedToolSpec {
                name: "aish.replay_show".into(),
                description: "show".into(),
                parameters: serde_json::json!({}),
                risk_class: ToolRiskClass::WriteLike,
                max_output_bytes: 8192,
            },
        ] {
            assert!(validate_client_tools(vec![spec]).is_err());
        }
    }
}
