//! Smart Preprocessor オーケストレーション（domain のみ依存）。

use crate::domain::smart_preprocessor::{
    derive_local_route_decision, run_preprocessor, should_short_circuit, should_use_local_route,
    LocalRouteDecision, PreprocessConfig, PreprocessInput, RouteMetadataInput,
    SmartPreprocessDecision,
};

#[derive(Debug, Clone)]
pub struct PreprocessorRunInput {
    pub query: String,
    pub command: String,
    pub tty: bool,
    pub new_conversation: bool,
    pub conversation_id: Option<String>,
    pub memory_enabled: bool,
    pub history_tail_summary: Option<String>,
    pub session_error_summary: Option<String>,
    pub cli_overrides: bool,
    pub route_metadata: RouteMetadataInput,
    pub history_id: Option<String>,
    pub model_load_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreprocessorRunOutcome {
    pub decision: SmartPreprocessDecision,
    pub short_circuit: bool,
    pub local_route: Option<LocalRouteDecision>,
    pub use_local_route: bool,
    pub history_id: Option<String>,
    pub model_load_error: Option<String>,
}

pub fn evaluate_preprocessor(
    input: PreprocessorRunInput,
    config: &PreprocessConfig,
    cli_tool_allowlist: &[String],
) -> PreprocessorRunOutcome {
    let preprocess_input = PreprocessInput {
        user_text: input.query.clone(),
        command: Some(input.command),
        tty: input.tty,
        new_conversation: input.new_conversation,
        conversation_id: input.conversation_id,
        memory_enabled: input.memory_enabled,
        history_tail_summary: input.history_tail_summary,
        session_error_summary: input.session_error_summary,
        cli_overrides: input.cli_overrides,
        route_metadata: input.route_metadata,
    };
    let decision = run_preprocessor(preprocess_input.clone(), config);
    let local_route =
        derive_local_route_decision(&decision, &input.query, config, cli_tool_allowlist);
    let use_local_route = local_route
        .as_ref()
        .map(|local| {
            should_use_local_route(&decision, local, config, input.tty, input.cli_overrides)
        })
        .unwrap_or(false);
    let short_circuit = should_short_circuit(&decision) && !use_local_route;
    PreprocessorRunOutcome {
        decision,
        short_circuit,
        local_route,
        use_local_route,
        history_id: input.history_id,
        model_load_error: input.model_load_error,
    }
}
