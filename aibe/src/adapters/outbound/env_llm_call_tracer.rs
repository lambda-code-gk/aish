//! `AIBE_LLM_TRACE=1` による LLM call site trace。

use crate::ports::outbound::LlmCallTracer;

#[derive(Debug, Default, Clone, Copy)]
pub struct EnvLlmCallTracer;

impl EnvLlmCallTracer {
    fn enabled() -> bool {
        std::env::var("AIBE_LLM_TRACE")
            .ok()
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
    }
}

impl LlmCallTracer for EnvLlmCallTracer {
    fn start(&self, site: &str, profile: Option<&str>, model: Option<&str>) {
        if !Self::enabled() {
            return;
        }
        let mut line = format!("aibe: llm_call start site={site}");
        if let Some(profile) = profile.filter(|value| !value.is_empty()) {
            line.push_str(&format!(" profile={profile}"));
        }
        if let Some(model) = model.filter(|value| !value.is_empty()) {
            line.push_str(&format!(" model={model}"));
        }
        eprintln!("{line}");
    }

    fn end(&self, site: &str, elapsed_ms: u64, ok: bool) {
        if !Self::enabled() {
            return;
        }
        eprintln!("aibe: llm_call end site={site} elapsed_ms={elapsed_ms} ok={ok}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_disabled_by_default() {
        std::env::remove_var("AIBE_LLM_TRACE");
        assert!(!EnvLlmCallTracer::enabled());
    }

    #[test]
    fn trace_end_ok_flag() {
        std::env::set_var("AIBE_LLM_TRACE", "1");
        let tracer = EnvLlmCallTracer;
        tracer.start("route_turn", Some("fast"), None);
        tracer.end("route_turn", 12, true);
        tracer.start("agent_turn", None, None);
        tracer.end("agent_turn", 34, true);
        std::env::remove_var("AIBE_LLM_TRACE");
    }
}
