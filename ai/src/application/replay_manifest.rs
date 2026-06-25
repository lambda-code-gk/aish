//! shell_log_mode 解決と replay manifest 生成。

use aibe_protocol::{ClientProvidedToolSpec, ToolRiskClass};
use aish_replay::{replay_span_views, sanitize_single_line_field, LogEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLogMode {
    Off,
    Tail,
    Manifest,
    Hybrid,
}

impl ShellLogMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()) {
            Some(mode) if mode == "off" => Self::Off,
            Some(mode) if mode == "tail" => Self::Tail,
            Some(mode) if mode == "manifest" => Self::Manifest,
            Some(mode) if mode == "hybrid" => Self::Hybrid,
            _ => Self::Hybrid,
        }
    }

    pub fn advertises_manifest(self) -> bool {
        matches!(self, Self::Manifest | Self::Hybrid)
    }

    pub fn advertises_tail(self) -> bool {
        matches!(self, Self::Tail | Self::Hybrid)
    }
}

pub fn render_manifest_block(events: &[LogEvent]) -> String {
    let views = replay_span_views(events).unwrap_or_default();
    let mut out = String::from("[replay manifest]\n");
    for view in views {
        out.push_str(&format!(
            "#{} exit={} command=\"{}\"\n",
            view.index,
            view.exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".to_string()),
            sanitize_single_line_field(&view.command)
        ));
    }
    out
}

pub fn replay_manifest_client_tools_enabled(enabled: bool) -> Vec<ClientProvidedToolSpec> {
    if !enabled {
        return Vec::new();
    }
    vec![ClientProvidedToolSpec {
        name: "aish.replay_show".to_string(),
        description: "Show recorded terminal output from the current replayable span.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "index": { "type": "integer" },
                "stream": { "type": "string", "enum": ["stdout", "stderr", "both"] },
                "tail_bytes": { "type": "integer", "minimum": 1, "maximum": 16384 }
            },
            "required": ["index"]
        }),
        risk_class: ToolRiskClass::ReadOnly,
        max_output_bytes: 32_768,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_log_mode_resolves_off_tail_manifest_hybrid() {
        assert_eq!(ShellLogMode::parse(Some("off")), ShellLogMode::Off);
        assert_eq!(ShellLogMode::parse(Some("tail")), ShellLogMode::Tail);
        assert_eq!(
            ShellLogMode::parse(Some("manifest")),
            ShellLogMode::Manifest
        );
        assert_eq!(ShellLogMode::parse(Some("hybrid")), ShellLogMode::Hybrid);
        assert_eq!(ShellLogMode::parse(Some("unknown")), ShellLogMode::Hybrid);
    }

    #[test]
    fn shell_log_mode_manifest_gate_matches_modes() {
        assert!(!ShellLogMode::Off.advertises_manifest());
        assert!(!ShellLogMode::Tail.advertises_manifest());
        assert!(ShellLogMode::Manifest.advertises_manifest());
        assert!(ShellLogMode::Hybrid.advertises_manifest());
    }

    #[test]
    fn replay_manifest_advertises_aish_replay_show() {
        let tools = replay_manifest_client_tools_enabled(true);
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(tool.name, "aish.replay_show");
        assert_eq!(tool.risk_class, ToolRiskClass::ReadOnly);
        assert!(tool.parameters.get("properties").is_some());
        assert!(tool.max_output_bytes > 0);
    }
}
