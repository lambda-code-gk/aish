//! shell_log_mode 解決と replay manifest 生成。

use std::path::Path;

use aibe_protocol::{ClientProvidedToolSpec, ToolRiskClass};
use aish_replay::{
    replay_manifest_entries, sanitize_single_line_field, LogEvent, ReplayManifestEntry,
};

use crate::domain::ShellLogChoice;

pub const DEFAULT_REPLAY_MANIFEST_LIMIT: usize = 30;
pub const DEFAULT_REPLAY_MANIFEST_PREVIEW_BYTES: usize = 160;

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

#[derive(Debug, Clone)]
pub struct TurnReplayContext {
    pub shell_log_tail: Option<String>,
    pub replay_events: Vec<LogEvent>,
    pub replay_manifest_block: Option<String>,
    pub client_tools: Vec<ClientProvidedToolSpec>,
    /// hybrid mode で manifest 読み込みに失敗したときの説明（eprintln 用）。
    pub manifest_fallback: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TurnReplayContextError {
    #[error("replay manifest required: {0}")]
    ManifestRequired(String),
    #[error("shell log tail read failed: {0}")]
    TailRead(String),
}

pub fn build_turn_replay_context(
    shell_log_mode: ShellLogMode,
    shell_log_choice: &ShellLogChoice,
    shell_log_override: Option<String>,
    read_tail: impl FnOnce() -> Result<Option<String>, String>,
    load_events: impl FnOnce(&Path) -> Result<Vec<LogEvent>, String>,
) -> Result<TurnReplayContext, TurnReplayContextError> {
    let shell_log_tail = if shell_log_mode.advertises_tail() {
        if let Some(text) = shell_log_override {
            Some(text)
        } else {
            read_tail().map_err(TurnReplayContextError::TailRead)?
        }
    } else {
        None
    };

    let mut manifest_fallback = None;
    let replay_events = if shell_log_mode.advertises_manifest() {
        match shell_log_choice {
            ShellLogChoice::Path(path) => match load_events(path) {
                Ok(events) => events,
                Err(err) => {
                    if shell_log_mode == ShellLogMode::Manifest {
                        return Err(TurnReplayContextError::ManifestRequired(err));
                    }
                    manifest_fallback = Some(err);
                    Vec::new()
                }
            },
            ShellLogChoice::None => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let replay_manifest_block = if replay_events.is_empty() {
        None
    } else {
        Some(render_manifest_block(&replay_events))
    };
    let client_tools = replay_manifest_client_tools_enabled(
        !replay_events.is_empty() && shell_log_mode.advertises_manifest(),
    );

    Ok(TurnReplayContext {
        shell_log_tail,
        replay_events,
        replay_manifest_block,
        client_tools,
        manifest_fallback,
    })
}

pub fn render_manifest_block(events: &[LogEvent]) -> String {
    let entries =
        replay_manifest_entries(events, DEFAULT_REPLAY_MANIFEST_PREVIEW_BYTES).unwrap_or_default();
    let start = entries.len().saturating_sub(DEFAULT_REPLAY_MANIFEST_LIMIT);
    let limited = &entries[start..];
    render_manifest_entries(limited)
}

fn render_manifest_entries(entries: &[ReplayManifestEntry]) -> String {
    let mut out = String::from("[replay manifest]\n");
    for entry in entries {
        out.push_str(&format!(
            "#{} exit={} stdout={}B stderr={}B failed={} command=\"{}\"",
            entry.index,
            entry
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".to_string()),
            entry.stdout_bytes,
            entry.stderr_bytes,
            entry.failed,
            sanitize_single_line_field(&entry.command),
        ));
        if !entry.stderr_preview.is_empty() {
            out.push_str(&format!(
                " stderr_preview=\"{}\"",
                sanitize_single_line_field(&entry.stderr_preview)
            ));
        }
        if !entry.stdout_preview.is_empty() {
            out.push_str(&format!(
                " stdout_preview=\"{}\"",
                sanitize_single_line_field(&entry.stdout_preview)
            ));
        }
        out.push('\n');
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
    use aish_replay::CommandKind;

    fn sample_events(count: usize) -> Vec<LogEvent> {
        (1..=count)
            .map(|index| {
                vec![
                    LogEvent::command_start_span(
                        &aish_replay::CommandSpec {
                            program: format!("cmd{index}"),
                            args: vec![],
                        },
                        index as u32,
                        "2026-01-01T00:00:00Z",
                        CommandKind::Exec,
                    ),
                    LogEvent::stdout_indexed(&format!("out{index}\n"), index as u32),
                    LogEvent::stderr_indexed(&format!("err{index}\n"), index as u32),
                    LogEvent::command_end(index as u32, Some(0), "2026-01-01T00:00:01Z"),
                ]
            })
            .flatten()
            .collect()
    }

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
        assert!(!ShellLogMode::Off.advertises_tail());
        assert!(ShellLogMode::Tail.advertises_tail());
        assert!(!ShellLogMode::Manifest.advertises_tail());
        assert!(ShellLogMode::Hybrid.advertises_tail());
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

    #[test]
    fn build_turn_replay_context_respects_shell_log_mode_matrix() {
        let path = Path::new("/tmp/session.log");
        let events = sample_events(1);
        let load_ok = |_: &Path| Ok(events.clone());
        let load_err = |_: &Path| Err("missing".into());
        let tail_ok = || Ok(Some("tail".into()));

        let off = build_turn_replay_context(
            ShellLogMode::Off,
            &ShellLogChoice::Path(path.into()),
            None,
            tail_ok,
            load_ok,
        )
        .expect("off");
        assert!(off.shell_log_tail.is_none());
        assert!(off.replay_events.is_empty());
        assert!(off.client_tools.is_empty());

        let tail = build_turn_replay_context(
            ShellLogMode::Tail,
            &ShellLogChoice::Path(path.into()),
            None,
            tail_ok,
            load_ok,
        )
        .expect("tail");
        assert_eq!(tail.shell_log_tail.as_deref(), Some("tail"));
        assert!(tail.replay_events.is_empty());
        assert!(tail.client_tools.is_empty());

        let manifest = build_turn_replay_context(
            ShellLogMode::Manifest,
            &ShellLogChoice::Path(path.into()),
            None,
            || Ok(None),
            load_ok,
        )
        .expect("manifest");
        assert!(manifest.shell_log_tail.is_none());
        assert!(!manifest.replay_events.is_empty());
        assert_eq!(manifest.client_tools.len(), 1);

        let hybrid = build_turn_replay_context(
            ShellLogMode::Hybrid,
            &ShellLogChoice::Path(path.into()),
            None,
            tail_ok,
            load_ok,
        )
        .expect("hybrid");
        assert_eq!(hybrid.shell_log_tail.as_deref(), Some("tail"));
        assert!(!hybrid.replay_events.is_empty());
        assert_eq!(hybrid.client_tools.len(), 1);

        let manifest_err = build_turn_replay_context(
            ShellLogMode::Manifest,
            &ShellLogChoice::Path(path.into()),
            None,
            || Ok(None),
            load_err,
        )
        .expect_err("manifest required");
        assert!(matches!(
            manifest_err,
            TurnReplayContextError::ManifestRequired(_)
        ));

        let hybrid_fallback = build_turn_replay_context(
            ShellLogMode::Hybrid,
            &ShellLogChoice::Path(path.into()),
            None,
            tail_ok,
            load_err,
        )
        .expect("hybrid fallback");
        assert_eq!(hybrid_fallback.shell_log_tail.as_deref(), Some("tail"));
        assert!(hybrid_fallback.replay_events.is_empty());
        assert!(hybrid_fallback.manifest_fallback.is_some());
    }

    #[test]
    fn render_manifest_block_limits_to_latest_spans_with_stderr_preview() {
        let events = sample_events(100);
        let block = render_manifest_block(&events);
        assert!(block.contains("stderr_preview="));
        assert!(block.contains("#71 "));
        assert!(!block.contains("#1 "));
        assert!(block.contains("stdout="));
        assert!(block.contains("stderr="));
    }
}
