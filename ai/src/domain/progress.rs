//! progress 表示（TTY デフォルト ON）の解決。

/// CLI / preset / config / TTY default の優先順位で progress を決める。
///
/// hardcoded default は **TTY なら true**、非 TTY なら false。
pub fn resolve_progress(
    cli: Option<bool>,
    preset: Option<bool>,
    config: Option<bool>,
    stderr_tty: bool,
) -> bool {
    match cli {
        Some(value) => value,
        None => match preset {
            Some(value) => value,
            None => match config {
                Some(value) => value,
                None => stderr_tty,
            },
        },
    }
}

/// progress event の phase / message をスピナー向け短いラベルにする。
pub fn format_progress_label(phase: &str, message: Option<&str>) -> String {
    match phase {
        "thinking" => match message {
            Some("planning tool round") => "thinking…".to_string(),
            Some("generating response") => "generating…".to_string(),
            Some(msg) => format!("thinking: {msg}"),
            None => "thinking…".to_string(),
        },
        "tool_call" => message
            .map(|name| format!("running {name}…"))
            .unwrap_or_else(|| "running tool…".to_string()),
        "waiting_approval" => "waiting for approval…".to_string(),
        "finalizing" => "finalizing…".to_string(),
        "cancelling" => "cancelling…".to_string(),
        other => message
            .map(|msg| format!("{other}: {msg}"))
            .unwrap_or_else(|| other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_preset_and_config() {
        assert!(!resolve_progress(Some(false), Some(true), Some(true), true));
        assert!(resolve_progress(Some(true), Some(false), None, false));
    }

    #[test]
    fn preset_overrides_config() {
        assert!(!resolve_progress(None, Some(false), Some(true), true));
    }

    #[test]
    fn tty_default_when_unset() {
        assert!(resolve_progress(None, None, None, true));
        assert!(!resolve_progress(None, None, None, false));
    }

    #[test]
    fn format_labels_are_short() {
        assert_eq!(
            format_progress_label("thinking", Some("planning tool round")),
            "thinking…"
        );
        assert_eq!(
            format_progress_label("tool_call", Some("read_file")),
            "running read_file…"
        );
    }
}
