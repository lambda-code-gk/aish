//! human shell プロンプト prefix（0055 Phase 5）。

use std::path::PathBuf;

use crate::human_shell::validate_handoff_id;

const DEFAULT_TEMPLATE: &str = "[collab:{state}] ";
const WAITING_TEMPLATE: &str = "[collab:waiting — run 'ai' to resume] ";

/// 協調 handoff 用プロンプト prefix。human-shell 以外では空。
pub fn render_collaborative_prompt_prefix() -> String {
    if std::env::var("AISH_CONTROL_MODE").as_deref() != Ok("human-shell") {
        return String::new();
    }
    let handoff_id = std::env::var("AISH_HANDOFF_ID").unwrap_or_default();
    if handoff_id.is_empty() || validate_handoff_id(&handoff_id).is_err() {
        return String::new();
    }
    let state = read_handoff_state(&handoff_id).unwrap_or_else(|| "human-active".into());
    if state == "SIDE_AGENT_WAITING_FOR_HUMAN" {
        return template_or_default("waiting", WAITING_TEMPLATE);
    }
    let label = state
        .strip_prefix("SIDE_AGENT_")
        .unwrap_or(&state)
        .to_ascii_lowercase()
        .replace('_', "-");
    let template = std::env::var("AISH_COLLABORATIVE_PROMPT_TEMPLATE")
        .unwrap_or_else(|_| DEFAULT_TEMPLATE.to_string());
    template.replace("{state}", &label)
}

fn template_or_default(key: &str, fallback: &str) -> String {
    std::env::var("AISH_COLLABORATIVE_PROMPT_TEMPLATE")
        .ok()
        .filter(|value| value.contains("{state}"))
        .map(|template| template.replace("{state}", key))
        .unwrap_or_else(|| fallback.to_string())
}

fn read_handoff_state(handoff_id: &str) -> Option<String> {
    let root = std::env::var_os("AISH_HANDOFF_STORE_ROOT")?;
    let path = PathBuf::from(root).join(handoff_id).join("handoff.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    value
        .get("state")
        .and_then(|state| state.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_human_shell_mode_returns_empty_prefix() {
        std::env::remove_var("AISH_CONTROL_MODE");
        assert!(render_collaborative_prompt_prefix().is_empty());
    }

    #[test]
    fn waiting_state_uses_waiting_template() {
        let tmp = tempfile::tempdir().unwrap();
        let handoff_dir = tmp.path().join("ho-test");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(
            handoff_dir.join("handoff.json"),
            r#"{"state":"SIDE_AGENT_WAITING_FOR_HUMAN"}"#,
        )
        .unwrap();
        unsafe {
            std::env::set_var("AISH_CONTROL_MODE", "human-shell");
            std::env::set_var("AISH_HANDOFF_ID", "ho-test");
            std::env::set_var("AISH_HANDOFF_STORE_ROOT", tmp.path());
            std::env::remove_var("AISH_COLLABORATIVE_PROMPT_TEMPLATE");
        }
        let prefix = render_collaborative_prompt_prefix();
        unsafe {
            std::env::remove_var("AISH_CONTROL_MODE");
            std::env::remove_var("AISH_HANDOFF_ID");
            std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
        }
        assert!(prefix.contains("waiting"));
        assert!(prefix.contains("ai"));
    }
}
