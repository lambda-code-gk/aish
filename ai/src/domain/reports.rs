//! `status` / `doctor` / `ping` / `dry-run` の表示用データ。

use serde::Serialize;

use super::console_hint::{
    console_hint_output_format_label, console_hint_source_label, console_hint_suppressed_by_label,
    ConsoleHintReport,
};
use super::output_format::{append_env_line, append_tsv_row, OutputFormat};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilterMetadata {
    pub enabled: bool,
    pub source: String,
    pub masked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollaborativeHandoffReport {
    pub handoff_id: String,
    pub parent_task: String,
    pub state: String,
    pub command_candidates: Vec<String>,
    pub resume_hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticsReport {
    pub command: String,
    pub config_socket_path: String,
    pub ask_default_profile: Option<String>,
    pub ask_filter: FilterMetadata,
    pub ask_tools: Vec<String>,
    pub socket_path: String,
    pub socket_alive: bool,
    pub socket_error: Option<String>,
    pub aish_session_dir: Option<String>,
    pub implicit_session_id: Option<String>,
    pub ai_ask_log: Option<String>,
    pub shell_log_choice: String,
    pub shell_log_path: Option<String>,
    pub shell_log_error: Option<String>,
    pub preset: Option<String>,
    pub log_tail_bytes: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collaborative_handoff: Vec<CollaborativeHandoffReport>,
}

impl DiagnosticsReport {
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Tsv => self.render_tsv(),
            OutputFormat::Json => {
                serde_json::to_string(self).expect("DiagnosticsReport serializes")
            }
            OutputFormat::Env => self.render_env(),
        }
    }

    fn render_tsv(&self) -> String {
        let mut out = String::new();
        append_tsv_row(&mut out, "command", &self.command);
        append_tsv_row(&mut out, "config.socket_path", &self.config_socket_path);
        append_tsv_row(
            &mut out,
            "config.ask_default_profile",
            self.ask_default_profile.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "config.ask_filter.enabled",
            if self.ask_filter.enabled {
                "true"
            } else {
                "false"
            },
        );
        append_tsv_row(
            &mut out,
            "config.ask_filter.source",
            &self.ask_filter.source,
        );
        append_tsv_row(
            &mut out,
            "config.ask_filter.masked",
            if self.ask_filter.masked {
                "true"
            } else {
                "false"
            },
        );
        append_tsv_row(&mut out, "config.ask_tools", &self.ask_tools.join(","));
        append_tsv_row(&mut out, "socket.path", &self.socket_path);
        append_tsv_row(
            &mut out,
            "socket.alive",
            if self.socket_alive { "true" } else { "false" },
        );
        append_tsv_row(
            &mut out,
            "socket.error",
            self.socket_error.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "session.aish_session_dir",
            self.aish_session_dir.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "session.implicit_session_id",
            self.implicit_session_id.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "session.ai_ask_log",
            self.ai_ask_log.as_deref().unwrap_or(""),
        );
        append_tsv_row(&mut out, "shell_log.choice", &self.shell_log_choice);
        append_tsv_row(
            &mut out,
            "shell_log.path",
            self.shell_log_path.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "shell_log.error",
            self.shell_log_error.as_deref().unwrap_or(""),
        );
        append_tsv_row(&mut out, "preset", self.preset.as_deref().unwrap_or(""));
        append_tsv_row(&mut out, "log_tail_bytes", &self.log_tail_bytes.to_string());
        for (index, handoff) in self.collaborative_handoff.iter().enumerate() {
            let prefix = format!("collaborative_handoff.{index}");
            append_tsv_row(&mut out, &format!("{prefix}.id"), &handoff.handoff_id);
            append_tsv_row(
                &mut out,
                &format!("{prefix}.parent_task"),
                &handoff.parent_task,
            );
            append_tsv_row(&mut out, &format!("{prefix}.state"), &handoff.state);
            append_tsv_row(
                &mut out,
                &format!("{prefix}.command_candidates"),
                &handoff.command_candidates.join(" | "),
            );
            append_tsv_row(
                &mut out,
                &format!("{prefix}.resume_hint"),
                &handoff.resume_hint,
            );
        }
        out
    }

    fn render_env(&self) -> String {
        let mut out = String::new();
        append_env_line(&mut out, "AI_COMMAND", &self.command);
        append_env_line(&mut out, "AI_CONFIG_SOCKET_PATH", &self.config_socket_path);
        append_env_line(
            &mut out,
            "AI_ASK_DEFAULT_PROFILE",
            self.ask_default_profile.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ASK_FILTER_ENABLED",
            if self.ask_filter.enabled {
                "true"
            } else {
                "false"
            },
        );
        append_env_line(&mut out, "AI_ASK_FILTER_SOURCE", &self.ask_filter.source);
        append_env_line(
            &mut out,
            "AI_ASK_FILTER_MASKED",
            if self.ask_filter.masked {
                "true"
            } else {
                "false"
            },
        );
        append_env_line(&mut out, "AI_ASK_TOOLS", &self.ask_tools.join(","));
        append_env_line(&mut out, "AI_SOCKET_PATH", &self.socket_path);
        append_env_line(
            &mut out,
            "AI_SOCKET_ALIVE",
            if self.socket_alive { "true" } else { "false" },
        );
        append_env_line(
            &mut out,
            "AI_SOCKET_ERROR",
            self.socket_error.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AISH_SESSION_DIR",
            self.aish_session_dir.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_IMPLICIT_SESSION_ID",
            self.implicit_session_id.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ASK_LOG",
            self.ai_ask_log.as_deref().unwrap_or(""),
        );
        append_env_line(&mut out, "AI_SHELL_LOG_CHOICE", &self.shell_log_choice);
        append_env_line(
            &mut out,
            "AI_SHELL_LOG_PATH",
            self.shell_log_path.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_SHELL_LOG_ERROR",
            self.shell_log_error.as_deref().unwrap_or(""),
        );
        append_env_line(&mut out, "AI_PRESET", self.preset.as_deref().unwrap_or(""));
        append_env_line(
            &mut out,
            "AI_LOG_TAIL_BYTES",
            &self.log_tail_bytes.to_string(),
        );
        if let Some(handoff) = self.collaborative_handoff.first() {
            append_env_line(&mut out, "AI_COLLABORATIVE_HANDOFF_ID", &handoff.handoff_id);
            append_env_line(&mut out, "AI_COLLABORATIVE_HANDOFF_STATE", &handoff.state);
            append_env_line(
                &mut out,
                "AI_COLLABORATIVE_RESUME_HINT",
                &handoff.resume_hint,
            );
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryRunReport {
    pub command: String,
    pub message_source: String,
    pub message_length: usize,
    pub message_masked: String,
    pub config_socket_path: String,
    pub ask_default_profile: Option<String>,
    pub ask_filter: FilterMetadata,
    pub ask_tools: Vec<String>,
    pub socket_path: String,
    pub aish_session_dir: Option<String>,
    pub implicit_session_id: Option<String>,
    pub ai_ask_log: Option<String>,
    pub shell_log_choice: String,
    pub shell_log_path: Option<String>,
    pub shell_log_error: Option<String>,
    pub dry_run: bool,
    pub preset: Option<String>,
    pub log_tail_bytes: usize,
    pub console_hint: ConsoleHintReport,
}

impl DryRunReport {
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Tsv => self.render_tsv(),
            OutputFormat::Json => serde_json::to_string(self).expect("DryRunReport serializes"),
            OutputFormat::Env => self.render_env(),
        }
    }

    fn render_tsv(&self) -> String {
        let mut out = String::new();
        append_tsv_row(&mut out, "command", &self.command);
        append_tsv_row(&mut out, "message.source", &self.message_source);
        append_tsv_row(&mut out, "message.length", &self.message_length.to_string());
        append_tsv_row(&mut out, "message.masked", &self.message_masked);
        append_tsv_row(&mut out, "config.socket_path", &self.config_socket_path);
        append_tsv_row(
            &mut out,
            "config.ask_default_profile",
            self.ask_default_profile.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "config.ask_filter.enabled",
            if self.ask_filter.enabled {
                "true"
            } else {
                "false"
            },
        );
        append_tsv_row(
            &mut out,
            "config.ask_filter.source",
            &self.ask_filter.source,
        );
        append_tsv_row(
            &mut out,
            "config.ask_filter.masked",
            if self.ask_filter.masked {
                "true"
            } else {
                "false"
            },
        );
        append_tsv_row(&mut out, "config.ask_tools", &self.ask_tools.join(","));
        append_tsv_row(&mut out, "socket.path", &self.socket_path);
        append_tsv_row(
            &mut out,
            "session.aish_session_dir",
            self.aish_session_dir.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "session.implicit_session_id",
            self.implicit_session_id.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "session.ai_ask_log",
            self.ai_ask_log.as_deref().unwrap_or(""),
        );
        append_tsv_row(&mut out, "shell_log.choice", &self.shell_log_choice);
        append_tsv_row(
            &mut out,
            "shell_log.path",
            self.shell_log_path.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "shell_log.error",
            self.shell_log_error.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "dry_run",
            if self.dry_run { "true" } else { "false" },
        );
        append_tsv_row(&mut out, "preset", self.preset.as_deref().unwrap_or(""));
        append_tsv_row(&mut out, "log_tail_bytes", &self.log_tail_bytes.to_string());
        append_console_hint_tsv(&mut out, &self.console_hint);
        out
    }

    fn render_env(&self) -> String {
        let mut out = String::new();
        append_env_line(&mut out, "AI_COMMAND", &self.command);
        append_env_line(&mut out, "AI_MESSAGE_SOURCE", &self.message_source);
        append_env_line(
            &mut out,
            "AI_MESSAGE_LENGTH",
            &self.message_length.to_string(),
        );
        append_env_line(&mut out, "AI_MESSAGE_MASKED", &self.message_masked);
        append_env_line(&mut out, "AI_CONFIG_SOCKET_PATH", &self.config_socket_path);
        append_env_line(
            &mut out,
            "AI_ASK_DEFAULT_PROFILE",
            self.ask_default_profile.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ASK_FILTER_ENABLED",
            if self.ask_filter.enabled {
                "true"
            } else {
                "false"
            },
        );
        append_env_line(&mut out, "AI_ASK_FILTER_SOURCE", &self.ask_filter.source);
        append_env_line(
            &mut out,
            "AI_ASK_FILTER_MASKED",
            if self.ask_filter.masked {
                "true"
            } else {
                "false"
            },
        );
        append_env_line(&mut out, "AI_ASK_TOOLS", &self.ask_tools.join(","));
        append_env_line(&mut out, "AI_SOCKET_PATH", &self.socket_path);
        append_env_line(
            &mut out,
            "AISH_SESSION_DIR",
            self.aish_session_dir.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_IMPLICIT_SESSION_ID",
            self.implicit_session_id.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ASK_LOG",
            self.ai_ask_log.as_deref().unwrap_or(""),
        );
        append_env_line(&mut out, "AI_SHELL_LOG_CHOICE", &self.shell_log_choice);
        append_env_line(
            &mut out,
            "AI_SHELL_LOG_PATH",
            self.shell_log_path.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_SHELL_LOG_ERROR",
            self.shell_log_error.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_DRY_RUN",
            if self.dry_run { "true" } else { "false" },
        );
        append_env_line(&mut out, "AI_PRESET", self.preset.as_deref().unwrap_or(""));
        append_env_line(
            &mut out,
            "AI_LOG_TAIL_BYTES",
            &self.log_tail_bytes.to_string(),
        );
        append_console_hint_env(&mut out, &self.console_hint);
        out
    }
}

fn append_console_hint_tsv(out: &mut String, hint: &ConsoleHintReport) {
    append_tsv_row(
        out,
        "console_hint.requested",
        if hint.requested { "true" } else { "false" },
    );
    append_tsv_row(
        out,
        "console_hint.source",
        console_hint_source_label(hint.source),
    );
    append_tsv_row(
        out,
        "console_hint.tty",
        if hint.tty { "true" } else { "false" },
    );
    append_tsv_row(
        out,
        "console_hint.output_format",
        console_hint_output_format_label(hint.output_format),
    );
    append_tsv_row(
        out,
        "console_hint.effective",
        if hint.effective { "true" } else { "false" },
    );
    append_tsv_row(
        out,
        "console_hint.suppressed_by",
        console_hint_suppressed_by_label(hint.suppressed_by),
    );
}

fn append_console_hint_env(out: &mut String, hint: &ConsoleHintReport) {
    append_env_line(
        out,
        "AI_CONSOLE_HINT_REQUESTED",
        if hint.requested { "true" } else { "false" },
    );
    append_env_line(
        out,
        "AI_CONSOLE_HINT_SOURCE",
        console_hint_source_label(hint.source),
    );
    append_env_line(
        out,
        "AI_CONSOLE_HINT_TTY",
        if hint.tty { "true" } else { "false" },
    );
    append_env_line(
        out,
        "AI_CONSOLE_HINT_OUTPUT_FORMAT",
        console_hint_output_format_label(hint.output_format),
    );
    append_env_line(
        out,
        "AI_CONSOLE_HINT_EFFECTIVE",
        if hint.effective { "true" } else { "false" },
    );
    append_env_line(
        out,
        "AI_CONSOLE_HINT_SUPPRESSED_BY",
        console_hint_suppressed_by_label(hint.suppressed_by),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::resolve_console_hints;

    #[test]
    fn dry_run_render_masks_filter_metadata_not_raw_filter() {
        let report = DryRunReport {
            command: "ask".into(),
            message_source: "argv".into(),
            message_length: 5,
            message_masked: "<masked>".into(),
            config_socket_path: "/tmp/aibe.sock".into(),
            ask_default_profile: Some("fast".into()),
            ask_filter: FilterMetadata {
                enabled: true,
                source: "config".into(),
                masked: true,
            },
            ask_tools: vec!["@read-only".into()],
            socket_path: "/tmp/aibe.sock".into(),
            aish_session_dir: None,
            implicit_session_id: None,
            ai_ask_log: None,
            shell_log_choice: "none".into(),
            shell_log_path: None,
            shell_log_error: None,
            dry_run: true,
            preset: None,
            log_tail_bytes: 16_384,
            console_hint: resolve_console_hints(None, None, None, false, None),
        };

        let json = serde_json::to_value(&report).expect("json");
        assert_eq!(json["ask_filter"]["enabled"], true);
        assert_eq!(json["ask_filter"]["source"], "config");
        assert_eq!(json["ask_filter"]["masked"], true);

        let env = report.render(OutputFormat::Env);
        assert!(env.contains("AI_ASK_FILTER_ENABLED='true'"));
        assert!(env.contains("AI_ASK_FILTER_SOURCE='config'"));
        assert!(env.contains("AI_ASK_FILTER_MASKED='true'"));
        assert!(!env.contains("raw_filter"));
        assert_eq!(json["console_hint"]["requested"], true);
        assert_eq!(json["console_hint"]["suppressed_by"], "tty");
        assert!(env.contains("AI_CONSOLE_HINT_REQUESTED='true'"));
        assert!(env.contains("AI_CONSOLE_HINT_SUPPRESSED_BY='tty'"));
    }
}
