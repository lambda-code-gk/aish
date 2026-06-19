//! Smart Preprocessor オーケストレーション（domain のみ依存）。

use crate::domain::smart_preprocessor::{
    run_preprocessor, should_short_circuit, PreprocessConfig, PreprocessInput, RouteMetadataInput,
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
    pub history_id: Option<String>,
    pub model_load_error: Option<String>,
}

pub fn evaluate_preprocessor(
    input: PreprocessorRunInput,
    config: &PreprocessConfig,
) -> PreprocessorRunOutcome {
    let preprocess_input = PreprocessInput {
        user_text: input.query,
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
    let decision = run_preprocessor(preprocess_input, config);
    let short_circuit = should_short_circuit(&decision);
    PreprocessorRunOutcome {
        decision,
        short_circuit,
        history_id: input.history_id,
        model_load_error: input.model_load_error,
    }
}
