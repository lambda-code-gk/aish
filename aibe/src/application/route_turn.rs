//! `route_turn` ユースケース。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aibe_protocol::{
    sanitize_readonly_advisory_tools, sanitize_readonly_advisory_tools_option, ClientResponse,
    ErrorCode, FeatureAction, RouteKind, RoutePlan, RouteTurnCliOverrides, RouteTurnConversation,
    RouteTurnPreprocessorHints, RouteTurnSession, RouteTurnStatus, KNOWN_TOOLS,
    SHELL_LOG_TAIL_MAX_BYTES,
};
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::application::llm_call_trace::trace_llm_result;
use crate::domain::{
    actions_equivalent, feature_action_schema_prompt, ChatMessage, FeatureEligibilityContext,
    FeatureRegistry, MessageRole,
};
use crate::ports::outbound::{
    ConversationStore, ConversationStoreError, LlmCallTracer, LlmError, ProfileRegistry,
    RouterConfig,
};

#[derive(Debug, thiserror::Error)]
pub enum RouteTurnError {
    #[error("route_turn failed: {0}")]
    Provider(String),
    #[error("route_turn response was not valid JSON: {0}")]
    InvalidResponse(String),
    #[error("route_turn store error: {0}")]
    Store(#[from] ConversationStoreError),
}

pub struct RouteTurnService {
    profile_registry: ProfileRegistry,
    router: RouterConfig,
    store: Arc<dyn ConversationStore>,
    feature_registry: FeatureRegistry,
    feature_eligibility: FeatureEligibilityContext,
    llm_tracer: Arc<dyn LlmCallTracer>,
}

impl RouteTurnService {
    pub fn new(
        profile_registry: ProfileRegistry,
        router: RouterConfig,
        store: Arc<dyn ConversationStore>,
        feature_registry: FeatureRegistry,
        feature_eligibility: FeatureEligibilityContext,
        llm_tracer: Arc<dyn LlmCallTracer>,
    ) -> Self {
        Self {
            profile_registry,
            router,
            store,
            feature_registry,
            feature_eligibility,
            llm_tracer,
        }
    }

    #[cfg(test)]
    pub fn new_without_features(
        profile_registry: ProfileRegistry,
        router: RouterConfig,
        store: Arc<dyn ConversationStore>,
    ) -> Self {
        Self::new(
            profile_registry,
            router,
            store,
            FeatureRegistry::empty(),
            FeatureEligibilityContext::default(),
            Arc::new(crate::ports::outbound::NoopLlmCallTracer),
        )
    }
}

impl RouteTurnService {
    pub async fn run(
        &self,
        id: String,
        query: String,
        cwd: String,
        session: RouteTurnSession,
        conversation: RouteTurnConversation,
        cli_overrides: RouteTurnCliOverrides,
    ) -> ClientResponse {
        match self
            .run_inner(id.clone(), query, cwd, session, conversation, cli_overrides)
            .await
        {
            Ok(plan) => ClientResponse::RouteTurnResult {
                id,
                status: RouteTurnStatus::Ok,
                plan,
            },
            Err(e) => ClientResponse::error(id, ErrorCode::InternalError, e.to_string()),
        }
    }

    async fn run_inner(
        &self,
        _id: String,
        query: String,
        cwd: String,
        session: RouteTurnSession,
        conversation: RouteTurnConversation,
        cli_overrides: RouteTurnCliOverrides,
    ) -> Result<RoutePlan, RouteTurnError> {
        let (llm, _) = self
            .profile_registry
            .resolve(Some(self.router.profile.as_str()))
            .map_err(RouteTurnError::Provider)?;

        let (conversation_id, generated_conversation) = resolve_conversation_id(
            self.store.as_ref(),
            &session.ai_session_id,
            conversation.conversation_id.as_deref(),
            conversation.new_conversation,
        )?;
        let recent_summary = if conversation.new_conversation {
            conversation.recent_summary.clone()
        } else {
            self.store
                .load_recent_summary(&session.ai_session_id, Some(&conversation_id))?
                .or(conversation.recent_summary.clone())
        };
        let prompt = build_route_messages(
            &query,
            &cwd,
            &session,
            &conversation,
            &cli_overrides,
            recent_summary.as_deref(),
            &self.feature_registry,
        );
        let profile_name = self.router.profile.as_str();
        let response = trace_llm_result(&self.llm_tracer, "route_turn", Some(profile_name), || {
            llm.complete(&prompt)
        })
        .await
        .map_err(|e| RouteTurnError::Provider(llm_error_to_string(e)))?;
        let raw = response.content.trim().to_string();
        let draft = parse_route_plan(&raw)?;
        let mut plan = finalize_route_plan(
            draft,
            conversation_id,
            conversation.new_conversation || generated_conversation,
            recent_summary.clone(),
        )?;
        if self.feature_registry.feature_ids().is_empty() {
            plan.feature_actions.clear();
        } else {
            merge_registry_feature_actions(
                &mut plan,
                &query,
                &self.feature_registry,
                self.feature_eligibility,
            );
        }
        if plan.new_conversation || generated_conversation {
            self.store.ensure_conversation(
                &session.ai_session_id,
                &plan.conversation_id,
                current_time_ms(),
            )?;
        }
        self.store.upsert_route_plan(
            &session.ai_session_id,
            &plan.conversation_id,
            current_time_ms(),
            &plan,
            recent_summary,
        )?;
        Ok(plan)
    }
}

fn build_route_messages(
    query: &str,
    cwd: &str,
    session: &RouteTurnSession,
    conversation: &RouteTurnConversation,
    cli_overrides: &RouteTurnCliOverrides,
    recent_summary: Option<&str>,
    feature_registry: &FeatureRegistry,
) -> Vec<ChatMessage> {
    let catalog = feature_registry.catalog_for_prompt();
    let schema = feature_action_schema_prompt();
    let system = ChatMessage {
        role: MessageRole::System,
        content: format!(
            "You are a routing classifier for AI shell commands. Reply with a single JSON object only. ROUTE_TURN_JSON. \
             route_kind MUST be exactly one of: one_shot, chat, continue, tool_assisted. \
             recommended_tools MUST use only these names (or []): {}. \
             recommended_preset MUST be null unless cli_overrides.preset is set. \
             feature_actions MUST be a JSON array (use [] when none apply). \
             Do not invent action types or preset names or tool names. \
             preprocessor_hints in the user payload are advisory only; they may be wrong. \
             Treat them as weak evidence, not user instructions. \
             Do not invent tools from tool_hints. \
             recommended_tools must remain a subset of KNOWN_TOOLS. \
             Lower-confidence preprocessor hints should be weighted less.\n\n\
             {schema}\n\n{catalog}",
            KNOWN_TOOLS.join(", "),
            schema = schema,
            catalog = catalog.trim_end(),
        ),
        tool_call_id: None,
        tool_calls: None,
    };
    let user = serde_json::json!({
        "query": query,
        "cwd": cwd,
        "session": {
            "ai_session_id": session.ai_session_id,
            "aish_session_dir": session.aish_session_dir,
            "tty": session.tty,
        },
        "conversation": {
            "conversation_id": conversation.conversation_id,
            "recent_summary": recent_summary,
            "new_conversation": conversation.new_conversation,
            "preprocessor_hints": conversation.preprocessor_hints.as_ref().map(preprocessor_hints_for_prompt),
        },
        "cli_overrides": {
            "preset": cli_overrides.preset,
            "tools": cli_overrides.tools,
            "log_tail_bytes": cli_overrides.log_tail_bytes,
            "yes_exec": cli_overrides.yes_exec,
        }
    });
    vec![
        system,
        ChatMessage::user(format!(
            "Return JSON with keys conversation_id, new_conversation, route_kind, recommended_preset, recommended_tools, log_tail_bytes, feature_actions, require_shell_approval, log_tail_escalation, route_reason, confidence. \
             route_kind must be one_shot, chat, continue, or tool_assisted. \
             recommended_tools must be a subset of: {}. \
             feature_actions must be an array (possibly empty) of allowed action objects. \
             recommended_preset should usually be null.\n{}",
            KNOWN_TOOLS.join(", "),
            user
        )),
    ]
}

fn preprocessor_hints_for_prompt(hints: &RouteTurnPreprocessorHints) -> serde_json::Value {
    serde_json::json!({
        "context_needs": hints.context_needs,
        "tool_hints": hints.tool_hints,
        "failure_kind": hints.failure_kind,
        "preprocessor_intent": hints.preprocessor_intent,
        "preprocessor_reason_codes": hints.preprocessor_reason_codes,
        "confidence_bps": hints.confidence_bps,
        "confidence_gate": hints.confidence_gate,
        "safety_requires_approval": hints.safety_requires_approval,
    })
}

#[derive(Debug, Deserialize)]
struct RoutePlanDraft {
    conversation_id: Option<String>,
    new_conversation: Option<bool>,
    route_kind: Option<String>,
    recommended_preset: Option<String>,
    recommended_tools: Option<Vec<String>>,
    log_tail_bytes: Option<u64>,
    require_shell_approval: Option<bool>,
    log_tail_escalation: Option<bool>,
    feature_actions: Option<JsonValue>,
    route_reason: Option<String>,
    confidence: Option<f32>,
}

fn parse_route_plan(raw: &str) -> Result<RoutePlanDraft, RouteTurnError> {
    let candidate = extract_json_object(raw)
        .ok_or_else(|| RouteTurnError::InvalidResponse("missing JSON object in response".into()))?;
    serde_json::from_str(candidate).map_err(|e| RouteTurnError::InvalidResponse(e.to_string()))
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    raw.get(start..=end)
}

fn clamp_log_tail_bytes(bytes: u64) -> u64 {
    let max = SHELL_LOG_TAIL_MAX_BYTES as u64;
    bytes.min(max)
}

fn merge_registry_feature_actions(
    plan: &mut RoutePlan,
    query: &str,
    registry: &FeatureRegistry,
    ctx: FeatureEligibilityContext,
) {
    for action in registry.match_eligible_actions(query, ctx) {
        if !plan
            .feature_actions
            .iter()
            .any(|existing| actions_equivalent(existing, &action))
        {
            plan.feature_actions.push(action);
        }
    }
}

fn finalize_route_plan(
    draft: RoutePlanDraft,
    conversation_id: String,
    new_conversation: bool,
    recent_summary: Option<String>,
) -> Result<RoutePlan, RouteTurnError> {
    let route_reason = draft
        .route_reason
        .or_else(|| recent_summary.clone())
        .unwrap_or_else(|| "route unavailable".to_string());
    let feature_actions = match draft.feature_actions {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| serde_json::from_value::<FeatureAction>(item.clone()).ok())
                    .filter_map(|a| match a {
                        FeatureAction::Unsupported => None,
                        FeatureAction::MemoryRecipeRun { apply, .. } if apply => None,
                        FeatureAction::SetRecommendedTools { tools } => {
                            let tools = sanitize_readonly_advisory_tools(&tools);
                            if tools.is_empty() {
                                None
                            } else {
                                Some(FeatureAction::SetRecommendedTools { tools })
                            }
                        }
                        other => Some(other),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    };
    Ok(RoutePlan {
        conversation_id: draft.conversation_id.unwrap_or(conversation_id),
        new_conversation: draft.new_conversation.unwrap_or(new_conversation),
        route_kind: normalize_route_kind(
            draft.route_kind.as_deref(),
            &draft.recommended_tools,
            new_conversation,
        ),
        recommended_preset: draft.recommended_preset.filter(|s| !s.trim().is_empty()),
        recommended_tools: sanitize_readonly_advisory_tools_option(draft.recommended_tools),
        log_tail_bytes: draft.log_tail_bytes.map(clamp_log_tail_bytes),
        require_shell_approval: draft.require_shell_approval.unwrap_or(false),
        log_tail_escalation: draft.log_tail_escalation.unwrap_or(false),
        route_reason: redact_route_reason(&route_reason),
        confidence: draft.confidence,
        feature_actions,
    })
}

fn resolve_conversation_id(
    store: &dyn ConversationStore,
    session_id: &str,
    conversation_id: Option<&str>,
    new_conversation: bool,
) -> Result<(String, bool), RouteTurnError> {
    if new_conversation {
        return Ok((next_conversation_id(), true));
    }
    if let Some(id) = conversation_id {
        return Ok((id.to_string(), false));
    }
    if let Some(latest) = store.latest_conversation_id(session_id)? {
        return Ok((latest, false));
    }
    Ok((next_conversation_id(), true))
}

fn next_conversation_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("conv-{:016x}{:08x}", current_time_ms(), seq)
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn redact_route_reason(reason: &str) -> String {
    let masked = mask_absolute_paths(reason);
    truncate_text(&masked, 200)
}

fn mask_absolute_paths(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '/' {
            out.push_str("<path>");
            while let Some(next) = chars.peek() {
                if next.is_whitespace() || matches!(next, ',' | ';' | ')' | '(' | '"' | '\'') {
                    break;
                }
                let _ = chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

fn normalize_route_kind(
    raw: Option<&str>,
    recommended_tools: &Option<Vec<String>>,
    new_conversation: bool,
) -> RouteKind {
    let has_tools = recommended_tools
        .as_ref()
        .is_some_and(|tools| !tools.is_empty());
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some("one_shot") => RouteKind::OneShot,
        Some("chat") => RouteKind::Chat,
        Some("continue") => RouteKind::Continue,
        Some("tool_assisted") => RouteKind::ToolAssisted,
        Some("local_tool" | "local_copilot" | "tool" | "tools" | "copilot") => {
            RouteKind::ToolAssisted
        }
        Some(other) => {
            let lower = other.to_ascii_lowercase();
            if lower.contains("tool") || lower.contains("copilot") {
                RouteKind::ToolAssisted
            } else if lower.contains("continue") && !new_conversation {
                RouteKind::Continue
            } else if lower.contains("chat") {
                RouteKind::Chat
            } else if has_tools {
                RouteKind::ToolAssisted
            } else {
                RouteKind::OneShot
            }
        }
        None if has_tools => RouteKind::ToolAssisted,
        None if !new_conversation => RouteKind::Continue,
        None => RouteKind::OneShot,
    }
}

fn llm_error_to_string(err: LlmError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessor_hints_do_not_change_final_route_kind() {
        let draft = RoutePlanDraft {
            conversation_id: None,
            new_conversation: None,
            route_kind: Some("chat".into()),
            recommended_preset: None,
            recommended_tools: None,
            log_tail_bytes: None,
            require_shell_approval: None,
            log_tail_escalation: None,
            feature_actions: None,
            route_reason: Some("inspect".into()),
            confidence: None,
        };
        let plan = finalize_route_plan(draft, "conv-1".into(), false, None).expect("finalize");
        assert_eq!(plan.route_kind, RouteKind::Chat);
    }

    #[test]
    fn route_turn_system_prompt_states_preprocessor_hints_are_advisory() {
        let messages = build_route_messages(
            "hello",
            "/tmp",
            &RouteTurnSession::default(),
            &RouteTurnConversation::default(),
            &RouteTurnCliOverrides::default(),
            None,
            &FeatureRegistry::empty(),
        );
        let system = &messages[0].content;
        assert!(system.contains("advisory"));
        assert!(system.contains("tool_hints"));
        assert!(system.contains("KNOWN_TOOLS"));
    }

    #[test]
    fn route_turn_preprocessor_hints_roundtrip_confidence_fields() {
        let hints = RouteTurnPreprocessorHints {
            context_needs: vec!["vcs_status".into()],
            tool_hints: vec!["git_status".into()],
            failure_kind: None,
            preprocessor_intent: Some("inspect".into()),
            preprocessor_reason_codes: vec!["vcs_context".into()],
            confidence_bps: Some(7200),
            confidence_gate: Some("assist_route_turn".into()),
            safety_requires_approval: Some(false),
        };
        let json = serde_json::to_string(&hints).expect("serialize");
        let back: RouteTurnPreprocessorHints = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.confidence_bps, Some(7200));
        assert_eq!(back.confidence_gate.as_deref(), Some("assist_route_turn"));
        assert_eq!(back.safety_requires_approval, Some(false));
    }

    #[test]
    fn route_turn_preprocessor_hints_deserialize_legacy_without_confidence_fields() {
        let legacy = r#"{
            "context_needs":["git_status"],
            "tool_hints":["git_status"],
            "preprocessor_intent":"inspect"
        }"#;
        let hints: RouteTurnPreprocessorHints = serde_json::from_str(legacy).expect("legacy");
        assert_eq!(hints.context_needs, vec!["git_status"]);
        assert!(hints.confidence_bps.is_none());
        assert!(hints.confidence_gate.is_none());
        assert!(hints.safety_requires_approval.is_none());
    }

    #[test]
    fn unknown_preprocessor_hints_are_ignored() {
        let hints = RouteTurnPreprocessorHints {
            context_needs: vec!["unknown_need".into()],
            tool_hints: vec!["unknown_tool".into()],
            failure_kind: Some("unknown_failure".into()),
            preprocessor_intent: Some("unknown_intent".into()),
            preprocessor_reason_codes: vec!["unknown_code".into()],
            ..Default::default()
        };
        let value = preprocessor_hints_for_prompt(&hints);
        assert_eq!(value["context_needs"][0], "unknown_need");
        let conversation = RouteTurnConversation {
            conversation_id: None,
            recent_summary: None,
            new_conversation: true,
            preprocessor_hints: Some(hints),
        };
        let messages = build_route_messages(
            "hello",
            "/tmp",
            &RouteTurnSession::default(),
            &conversation,
            &RouteTurnCliOverrides::default(),
            None,
            &FeatureRegistry::empty(),
        );
        assert!(messages.len() >= 2);
        assert!(messages[1].content.contains("preprocessor_hints"));
    }

    #[test]
    fn parse_route_plan_accepts_unknown_route_kind_strings() {
        let raw = r#"{"route_kind":"local_copilot","recommended_tools":["read_file"],"route_reason":"read repo"}"#;
        let draft = parse_route_plan(raw).expect("parse");
        let plan = finalize_route_plan(draft, "conv-1".into(), false, None).expect("finalize");
        assert_eq!(plan.route_kind, RouteKind::ToolAssisted);
    }

    #[test]
    fn normalize_route_kind_maps_local_tool() {
        assert_eq!(
            normalize_route_kind(Some("local_tool"), &None, true),
            RouteKind::ToolAssisted
        );
    }

    #[test]
    fn normalize_route_kind_defaults_to_continue_when_unset_and_not_new() {
        assert_eq!(
            normalize_route_kind(None, &None, false),
            RouteKind::Continue
        );
    }

    #[test]
    fn finalize_route_plan_sanitizes_recommended_tools_to_read_only() {
        let draft = RoutePlanDraft {
            conversation_id: None,
            new_conversation: None,
            route_kind: Some("tool_assisted".into()),
            recommended_preset: None,
            recommended_tools: Some(vec![
                "read_file".into(),
                "shell_exec".into(),
                "shell".into(),
                "grep".into(),
            ]),
            log_tail_bytes: None,
            require_shell_approval: None,
            log_tail_escalation: None,
            feature_actions: None,
            route_reason: Some("inspect".into()),
            confidence: None,
        };
        let plan = finalize_route_plan(draft, "conv-1".into(), false, None).expect("finalize");
        assert_eq!(
            plan.recommended_tools,
            Some(vec!["read_file".to_string(), "grep".to_string()])
        );
    }

    #[test]
    fn sanitize_readonly_advisory_tools_maps_view_file_to_read_file() {
        let got =
            sanitize_readonly_advisory_tools_option(Some(vec!["view_file".into()])).expect("tools");
        assert_eq!(got, vec!["read_file".to_string()]);
    }

    #[test]
    fn finalize_route_plan_clamps_log_tail_bytes_to_protocol_max() {
        let draft = RoutePlanDraft {
            conversation_id: None,
            new_conversation: None,
            route_kind: Some("continue".into()),
            recommended_preset: None,
            recommended_tools: None,
            log_tail_bytes: Some((SHELL_LOG_TAIL_MAX_BYTES as u64) + 999_999),
            require_shell_approval: None,
            log_tail_escalation: None,
            feature_actions: None,
            route_reason: Some("inspect".into()),
            confidence: None,
        };
        let plan = finalize_route_plan(draft, "conv-1".into(), false, None).expect("finalize");
        assert_eq!(plan.log_tail_bytes, Some(SHELL_LOG_TAIL_MAX_BYTES as u64));
    }

    #[test]
    fn finalize_route_plan_filters_invalid_feature_actions_best_effort() {
        let draft = RoutePlanDraft {
            conversation_id: None,
            new_conversation: None,
            route_kind: Some("tool_assisted".into()),
            recommended_preset: None,
            recommended_tools: None,
            log_tail_bytes: None,
            require_shell_approval: None,
            log_tail_escalation: None,
            feature_actions: Some(serde_json::json!([
                { "type": "memory_query" },
                { "type": "memory_recipe_run", "recipe_id": "recipe-a", "apply": true },
                { "type": "set_recommended_tools", "tools": ["read_file"] },
                { "type": "unknown_action", "foo": 1 }
            ])),
            route_reason: Some("read repo".into()),
            confidence: None,
        };
        let plan = finalize_route_plan(draft, "conv-1".into(), false, None).expect("finalize");
        assert_eq!(plan.feature_actions.len(), 2);
        assert!(matches!(
            plan.feature_actions[0],
            FeatureAction::MemoryQuery { .. }
        ));
        assert!(matches!(
            plan.feature_actions[1],
            FeatureAction::SetRecommendedTools { .. }
        ));
    }
}
