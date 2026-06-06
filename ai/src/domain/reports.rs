//! `status` / `doctor` / `ping` / `dry-run` の表示用データ。

use serde::Serialize;

use super::output_format::{append_env_line, append_tsv_row, OutputFormat};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticsReport {
    pub command: String,
    pub config_socket_path: String,
    pub ask_default_profile: Option<String>,
    pub ask_filter: Option<String>,
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
            "config.ask_filter",
            self.ask_filter.as_deref().unwrap_or(""),
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
            "AI_ASK_FILTER",
            self.ask_filter.as_deref().unwrap_or(""),
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
    pub ask_filter: Option<String>,
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
            "config.ask_filter",
            self.ask_filter.as_deref().unwrap_or(""),
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
            "AI_ASK_FILTER",
            self.ask_filter.as_deref().unwrap_or(""),
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
        out
    }
}
