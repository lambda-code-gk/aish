//! LLM 呼び出し site の trace port（実装は adapters 側）。

/// `route_turn` / `agent_turn` / `tool_round` / `memory_recipe` 等の LLM 呼び出しを観測する。
pub trait LlmCallTracer: Send + Sync {
    fn start(&self, site: &str, profile: Option<&str>, model: Option<&str>);
    fn end(&self, site: &str, elapsed_ms: u64, ok: bool);
}

/// trace 無効時の no-op 実装。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopLlmCallTracer;

impl LlmCallTracer for NoopLlmCallTracer {
    fn start(&self, _site: &str, _profile: Option<&str>, _model: Option<&str>) {}

    fn end(&self, _site: &str, _elapsed_ms: u64, _ok: bool) {}
}
