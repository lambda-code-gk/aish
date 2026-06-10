//! コンソールヒント（TTY 向け system instruction）の解決。

use super::output_format::OutputFormat;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleHintSource {
    Cli,
    Preset,
    Config,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleHintOutputFormat {
    Json,
    Tsv,
    Env,
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleHintSuppressedBy {
    Tty,
    Format,
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsoleHintReport {
    pub requested: bool,
    pub source: ConsoleHintSource,
    pub tty: bool,
    pub output_format: ConsoleHintOutputFormat,
    pub effective: bool,
    pub suppressed_by: ConsoleHintSuppressedBy,
}

/// CLI / preset / config / default の優先順位で `requested` を決め、TTY と format で `effective` を抑止する。
pub fn resolve_console_hints(
    cli: Option<bool>,
    preset: Option<bool>,
    config: Option<bool>,
    tty: bool,
    output_format: Option<OutputFormat>,
) -> ConsoleHintReport {
    let (requested, source) = match cli {
        Some(value) => (value, ConsoleHintSource::Cli),
        None => match preset {
            Some(value) => (value, ConsoleHintSource::Preset),
            None => match config {
                Some(value) => (value, ConsoleHintSource::Config),
                None => (true, ConsoleHintSource::Default),
            },
        },
    };

    let output_format_label = match output_format {
        Some(OutputFormat::Json) => ConsoleHintOutputFormat::Json,
        Some(OutputFormat::Tsv) => ConsoleHintOutputFormat::Tsv,
        Some(OutputFormat::Env) => ConsoleHintOutputFormat::Env,
        None => ConsoleHintOutputFormat::None,
    };

    let effective = requested && tty && output_format.is_none();
    let suppressed_by = if effective || !requested {
        ConsoleHintSuppressedBy::None
    } else if !tty {
        ConsoleHintSuppressedBy::Tty
    } else {
        ConsoleHintSuppressedBy::Format
    };

    ConsoleHintReport {
        requested,
        source,
        tty,
        output_format: output_format_label,
        effective,
        suppressed_by,
    }
}

pub(crate) fn console_hint_source_label(source: ConsoleHintSource) -> &'static str {
    match source {
        ConsoleHintSource::Cli => "cli",
        ConsoleHintSource::Preset => "preset",
        ConsoleHintSource::Config => "config",
        ConsoleHintSource::Default => "default",
    }
}

pub(crate) fn console_hint_output_format_label(format: ConsoleHintOutputFormat) -> &'static str {
    match format {
        ConsoleHintOutputFormat::Json => "json",
        ConsoleHintOutputFormat::Tsv => "tsv",
        ConsoleHintOutputFormat::Env => "env",
        ConsoleHintOutputFormat::None => "none",
    }
}

pub(crate) fn console_hint_suppressed_by_label(by: ConsoleHintSuppressedBy) -> &'static str {
    match by {
        ConsoleHintSuppressedBy::Tty => "tty",
        ConsoleHintSuppressedBy::Format => "format",
        ConsoleHintSuppressedBy::None => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_preset_and_config() {
        let report = resolve_console_hints(Some(false), Some(true), Some(true), true, None);
        assert!(!report.requested);
        assert_eq!(report.source, ConsoleHintSource::Cli);
    }

    #[test]
    fn preset_overrides_config() {
        let report = resolve_console_hints(None, Some(false), Some(true), true, None);
        assert!(!report.requested);
        assert_eq!(report.source, ConsoleHintSource::Preset);
    }

    #[test]
    fn config_used_when_cli_and_preset_none() {
        let report = resolve_console_hints(None, None, Some(false), true, None);
        assert!(!report.requested);
        assert_eq!(report.source, ConsoleHintSource::Config);
    }

    #[test]
    fn default_true_when_unset() {
        let report = resolve_console_hints(None, None, None, true, None);
        assert!(report.requested);
        assert_eq!(report.source, ConsoleHintSource::Default);
        assert!(report.effective);
    }

    #[test]
    fn non_tty_suppresses_with_reason_tty() {
        let report = resolve_console_hints(None, None, None, false, None);
        assert!(report.requested);
        assert!(!report.effective);
        assert_eq!(report.suppressed_by, ConsoleHintSuppressedBy::Tty);
    }

    #[test]
    fn format_suppresses_with_reason_format() {
        let report = resolve_console_hints(Some(true), None, None, true, Some(OutputFormat::Json));
        assert!(report.requested);
        assert!(!report.effective);
        assert_eq!(report.suppressed_by, ConsoleHintSuppressedBy::Format);
        assert_eq!(report.output_format, ConsoleHintOutputFormat::Json);
    }

    #[test]
    fn requested_false_has_no_suppression_reason() {
        let report = resolve_console_hints(Some(false), None, None, true, None);
        assert!(!report.requested);
        assert!(!report.effective);
        assert_eq!(report.suppressed_by, ConsoleHintSuppressedBy::None);
    }
}
