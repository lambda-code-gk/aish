//! Smart Preprocessor turn trace（`AI_SMART_PREPROCESSOR_TRACE=1` / `--trace-route`）。

use crate::adapters::outbound::smart_preprocessor_observation::{
    ObservationContext, TurnLlmAccounting,
};
use crate::domain::smart_preprocessor::{LocalRouteDecision, SmartPreprocessDecision};

pub fn smart_preprocessor_trace_enabled(cli_trace_route: bool) -> bool {
    if cli_trace_route {
        return true;
    }
    std::env::var("AI_SMART_PREPROCESSOR_TRACE")
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

pub fn emit_smart_preprocessor_trace(
    decision: &SmartPreprocessDecision,
    ctx: &ObservationContext,
    local_route: Option<&LocalRouteDecision>,
    turn_llm: &TurnLlmAccounting,
) {
    let fallback_required = local_route
        .map(|local| local.fallback_required)
        .unwrap_or(false);
    let fallback_reason = ctx
        .fallback_reason
        .as_deref()
        .or_else(|| {
            if fallback_required {
                local_route.and_then(|local| local.fallback_reason.as_deref())
            } else {
                None
            }
        })
        .unwrap_or("none");
    let local_route_kind = ctx
        .local_route
        .local_route_kind
        .as_deref()
        .or_else(|| local_route.map(|local| local.route_kind.as_str()))
        .unwrap_or("none");
    eprintln!("ai: smart_preprocessor:");
    eprintln!("  mode={}", decision.mode.as_str());
    eprintln!("  intent={}", decision.intent.as_str());
    eprintln!("  confidence_bps={}", decision.confidence_bps);
    eprintln!("  gate={}", decision.gate.as_str());
    eprintln!("  local_route_kind={local_route_kind}");
    eprintln!("  local_route_used={}", ctx.local_route.local_route_used);
    eprintln!("  fallback_required={fallback_required}");
    eprintln!("  fallback_reason={fallback_reason}");
    eprintln!("  route_turn_used={}", turn_llm.route_turn_used);
    eprintln!("  route_turn_latency_ms={}", turn_llm.route_turn_latency_ms);
    eprintln!("  agent_turn_used={}", turn_llm.agent_turn_used);
    eprintln!("  agent_turn_latency_ms={}", turn_llm.agent_turn_latency_ms);
    eprintln!("  total_turn_latency_ms={}", turn_llm.total_turn_latency_ms);
    eprintln!(
        "  llm_call_count_estimated={}",
        turn_llm.llm_call_count_estimated
    );
    if !turn_llm.llm_call_sites.is_empty() {
        eprintln!("  llm_call_sites={}", turn_llm.llm_call_sites.join(","));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::smart_preprocessor_observation::LocalRouteMetrics;
    use crate::domain::smart_preprocessor::{
        run_preprocessor, PreprocessConfig, PreprocessInput, RouteMetadataInput,
        SmartPreprocessMode,
    };

    #[test]
    fn trace_enabled_from_env() {
        std::env::set_var("AI_SMART_PREPROCESSOR_TRACE", "1");
        assert!(smart_preprocessor_trace_enabled(false));
        std::env::remove_var("AI_SMART_PREPROCESSOR_TRACE");
    }

    #[test]
    fn trace_enabled_from_cli() {
        std::env::remove_var("AI_SMART_PREPROCESSOR_TRACE");
        assert!(smart_preprocessor_trace_enabled(true));
    }

    #[test]
    fn emit_trace_includes_route_turn_used_false() {
        std::env::set_var("AI_SMART_PREPROCESSOR_TRACE", "1");
        let decision = run_preprocessor(
            PreprocessInput {
                user_text: "hello".into(),
                command: Some("ask".into()),
                tty: true,
                new_conversation: true,
                conversation_id: None,
                memory_enabled: true,
                history_tail_summary: None,
                session_error_summary: None,
                cli_overrides: false,
                route_metadata: RouteMetadataInput::default(),
            },
            &PreprocessConfig {
                mode: SmartPreprocessMode::Gate,
                ..PreprocessConfig::default()
            },
        );
        let ctx = ObservationContext {
            ai_session_id: Some("sess".into()),
            conversation_id: None,
            history_id: None,
            decision_path: "local_route".into(),
            route_turn_used: false,
            route_turn_hints_present: false,
            route_turn_hints_injected: false,
            fallback_reason: None,
            local_route: LocalRouteMetrics {
                local_route_kind: Some("simple_chat".into()),
                local_route_used: true,
                route_turn_skipped_count: 1,
                ..LocalRouteMetrics::default()
            },
        };
        let turn_llm = TurnLlmAccounting {
            route_turn_used: false,
            agent_turn_used: true,
            route_turn_latency_ms: 0,
            agent_turn_latency_ms: 12,
            total_turn_latency_ms: 20,
            llm_call_count_estimated: 1,
            llm_call_sites: vec!["agent_turn".into()],
        };
        emit_smart_preprocessor_trace(&decision, &ctx, None, &turn_llm);
        std::env::remove_var("AI_SMART_PREPROCESSOR_TRACE");
    }
}
