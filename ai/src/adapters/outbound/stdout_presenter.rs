//! 標準出力プレゼンター。

use std::io::Write;

use aibe_protocol::{
    AgentTurnStatus, ClientResponse, ExecutedToolCall, ExecutedToolStatus, MAX_TOOL_OUTPUT_BYTES,
};

use crate::domain::ToolsStartupLine;
use crate::domain::{append_env_line, append_tsv_row, OutputFormat};
use crate::ports::outbound::Presenter;

use super::output_filter::{
    apply_output_filter, format_filter_exit_status, write_filter_streams, FilterRunOutcome,
};

#[derive(Debug, Clone, Default)]
pub struct StdoutPresenter {
    output_filter: Option<String>,
    output_format: Option<OutputFormat>,
    quiet: bool,
}

/// `show_response` が書き込む内容（テスト・契約検証用）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresenterOutput {
    pub stdout: Option<String>,
    pub stderr: Vec<String>,
}

impl StdoutPresenter {
    pub fn new(output_filter: Option<String>) -> Self {
        Self {
            output_filter,
            output_format: None,
            quiet: false,
        }
    }

    pub fn with_options(
        output_filter: Option<String>,
        output_format: Option<OutputFormat>,
        quiet: bool,
    ) -> Self {
        Self {
            output_filter,
            output_format,
            quiet,
        }
    }

    pub fn is_quiet(&self) -> bool {
        self.quiet
    }

    fn emit_assistant_stdout(&self, content: &str) {
        if content.is_empty() {
            return;
        }
        if let Some(filter) = &self.output_filter {
            self.emit_filtered_stdout(content, filter);
        } else {
            println!("{content}");
        }
    }

    fn emit_filtered_stdout(&self, content: &str, filter: &str) {
        match apply_output_filter(content, filter) {
            FilterRunOutcome::Success { stdout, stderr } => {
                let _ = write_filter_streams(&stdout, &stderr);
            }
            FilterRunOutcome::NonZeroExit {
                status,
                stdout,
                stderr,
            } => {
                eprintln!(
                    "warning: ai: filter exited with status {}",
                    format_filter_exit_status(&status)
                );
                let _ = write_filter_streams(&stdout, &stderr);
            }
            FilterRunOutcome::SpawnFailed { message, stderr } => {
                eprintln!("warning: ai: filter failed: {message}");
                println!("{content}");
                if !stderr.is_empty() {
                    let _ = std::io::stderr().write_all(&stderr);
                }
            }
        }
    }
}

impl Presenter for StdoutPresenter {
    fn show_tools_startup(&self, line: &ToolsStartupLine) {
        if self.quiet {
            return;
        }
        eprintln!("{}", format_tools_startup(line));
    }

    fn show_external_commands(&self, names: &[String]) {
        if self.quiet {
            return;
        }
        if names.is_empty() {
            return;
        }
        eprintln!(
            "warning: ai: external commands registered: {}",
            names.join(",")
        );
    }

    fn show_progress(&self, phase: &str, message: Option<&str>) {
        if self.quiet {
            return;
        }
        match message {
            Some(message) => eprintln!("ai: progress: {phase}: {message}"),
            None => eprintln!("ai: progress: {phase}"),
        }
    }

    fn show_stream_chunk(&self, chunk: &str) {
        if self.quiet {
            return;
        }
        if !chunk.is_empty() {
            print!("{chunk}");
            let _ = std::io::stdout().flush();
        }
    }

    fn show_response(&self, response: &ClientResponse, verbose_tools: bool, streamed: bool) {
        let out = if let Some(format) = self.output_format {
            render_response_structured(
                response,
                verbose_tools,
                format,
                self.output_filter.as_deref(),
            )
        } else {
            render_response(response, verbose_tools)
        };
        if let Some(s) = out.stdout.as_deref() {
            if self.output_format.is_some() {
                println!("{s}");
            } else if !streamed {
                self.emit_assistant_stdout(s);
            }
        }
        if streamed && self.output_format.is_none() && !self.quiet {
            ensure_stdout_newline();
        }
        if !self.quiet {
            for line in out.stderr {
                eprintln!("{line}");
            }
        }
    }

    fn show_error(&self, message: &str) {
        eprintln!("ai: {message}");
    }
}

fn ensure_stdout_newline() {
    let _ = std::io::stdout().write_all(b"\n");
    let _ = std::io::stdout().flush();
}

pub fn format_tools_startup(line: &ToolsStartupLine) -> String {
    let prefix = if line.warn_shell { "warning: " } else { "" };
    match &line.source_hint {
        Some(hint) => format!("{prefix}ai: tools enabled: {} ({hint})", line.enabled_list),
        None => format!("{prefix}ai: tools enabled: {}", line.enabled_list),
    }
}

pub fn render_response(response: &ClientResponse, verbose_tools: bool) -> PresenterOutput {
    match response {
        ClientResponse::AgentTurnResult {
            status,
            assistant_message,
            tool_calls,
            ..
        } => {
            let mut stderr = Vec::new();
            if *status == AgentTurnStatus::MaxToolRounds {
                stderr.push(
                    "warning: ai: max tool rounds reached; showing partial assistant reply"
                        .to_string(),
                );
            }
            if verbose_tools {
                for tc in tool_calls {
                    stderr.push(format_tool_call_line(tc));
                }
            }
            let stdout = if assistant_message.content.is_empty() {
                None
            } else {
                Some(assistant_message.content.clone())
            };
            PresenterOutput { stdout, stderr }
        }
        ClientResponse::Pong { id } => PresenterOutput {
            stdout: None,
            stderr: vec![format!("ai: pong ({id})")],
        },
        ClientResponse::Error { message, .. } => PresenterOutput {
            stdout: None,
            stderr: vec![format!("aibe error: {message}")],
        },
        ClientResponse::ShellExecApprovalPrompt { .. } => PresenterOutput {
            stdout: None,
            stderr: vec!["ai: internal error: unexpected shell_exec approval prompt".into()],
        },
        ClientResponse::Cancelled { reason, .. } => PresenterOutput {
            stdout: None,
            stderr: vec![match reason {
                Some(reason) => format!("ai: cancelled: {reason}"),
                None => "ai: cancelled".to_string(),
            }],
        },
        ClientResponse::Progress { .. } | ClientResponse::AssistantStreaming { .. } => {
            PresenterOutput {
                stdout: None,
                stderr: Vec::new(),
            }
        }
        ClientResponse::RouteTurnResult { .. } => PresenterOutput {
            stdout: None,
            stderr: Vec::new(),
        },
    }
}

pub fn render_response_structured(
    response: &ClientResponse,
    verbose_tools: bool,
    format: OutputFormat,
    output_filter: Option<&str>,
) -> PresenterOutput {
    let mut view = ResponseView::from_response(response, verbose_tools, output_filter);
    let stdout = match format {
        OutputFormat::Json => serde_json::to_string(&view).ok(),
        OutputFormat::Tsv => Some(view.render_tsv()),
        OutputFormat::Env => Some(view.render_env()),
    };
    let mut stderr = Vec::new();
    if view.warn_max_tool_rounds {
        stderr.push(
            "warning: ai: max tool rounds reached; showing partial assistant reply".to_string(),
        );
    }
    stderr.append(&mut view.filter_warnings);
    stderr.append(&mut view.filter_stderr);
    stderr.append(&mut view.tool_warnings);
    PresenterOutput { stdout, stderr }
}

#[derive(Debug, Clone, serde::Serialize)]
struct ResponseView {
    response_type: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assistant_message: Option<aibe_protocol::ProtocolMessageOut>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ExecutedToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alive: Option<bool>,
    #[serde(skip)]
    warn_max_tool_rounds: bool,
    #[serde(skip)]
    filter_warnings: Vec<String>,
    #[serde(skip)]
    filter_stderr: Vec<String>,
    #[serde(skip)]
    tool_warnings: Vec<String>,
}

impl ResponseView {
    fn from_response(
        response: &ClientResponse,
        verbose_tools: bool,
        output_filter: Option<&str>,
    ) -> Self {
        match response {
            ClientResponse::AgentTurnResult {
                id,
                status,
                assistant_message,
                tool_calls,
                ..
            } => {
                let mut assistant_message = assistant_message.clone();
                let mut filter_warnings = Vec::new();
                let mut filter_stderr = Vec::new();
                if let Some(filter) = output_filter {
                    match apply_output_filter(&assistant_message.content, filter) {
                        FilterRunOutcome::Success { stdout, stderr } => {
                            assistant_message.content =
                                String::from_utf8_lossy(&stdout).into_owned();
                            if !stderr.is_empty() {
                                filter_stderr.push(String::from_utf8_lossy(&stderr).into_owned());
                            }
                        }
                        FilterRunOutcome::NonZeroExit {
                            status,
                            stdout,
                            stderr,
                        } => {
                            filter_warnings.push(format!(
                                "warning: ai: filter exited with status {}",
                                format_filter_exit_status(&status)
                            ));
                            assistant_message.content =
                                String::from_utf8_lossy(&stdout).into_owned();
                            if !stderr.is_empty() {
                                filter_stderr.push(String::from_utf8_lossy(&stderr).into_owned());
                            }
                        }
                        FilterRunOutcome::SpawnFailed { message, stderr } => {
                            filter_warnings.push(format!("warning: ai: filter failed: {message}"));
                            if !stderr.is_empty() {
                                filter_stderr.push(String::from_utf8_lossy(&stderr).into_owned());
                            }
                        }
                    }
                }
                let mut tool_warnings = Vec::new();
                if verbose_tools {
                    for tc in tool_calls {
                        tool_warnings.push(format_tool_call_line(tc));
                    }
                }
                Self {
                    response_type: "agent_turn_result".to_string(),
                    id: id.clone(),
                    status: Some(match status {
                        AgentTurnStatus::Ok => "ok".to_string(),
                        AgentTurnStatus::MaxToolRounds => "max_tool_rounds".to_string(),
                    }),
                    assistant_message: Some(assistant_message),
                    tool_calls: tool_calls.clone(),
                    error_code: None,
                    error_message: None,
                    alive: None,
                    warn_max_tool_rounds: *status == AgentTurnStatus::MaxToolRounds,
                    filter_warnings,
                    filter_stderr,
                    tool_warnings,
                }
            }
            ClientResponse::Pong { id } => Self {
                response_type: "pong".to_string(),
                id: id.clone(),
                status: None,
                assistant_message: None,
                tool_calls: Vec::new(),
                error_code: None,
                error_message: None,
                alive: Some(true),
                warn_max_tool_rounds: false,
                filter_warnings: Vec::new(),
                filter_stderr: Vec::new(),
                tool_warnings: Vec::new(),
            },
            ClientResponse::Error { id, code, message } => Self {
                response_type: "error".to_string(),
                id: id.clone(),
                status: None,
                assistant_message: None,
                tool_calls: Vec::new(),
                error_code: Some(format!("{code:?}")),
                error_message: Some(message.clone()),
                alive: None,
                warn_max_tool_rounds: false,
                filter_warnings: Vec::new(),
                filter_stderr: Vec::new(),
                tool_warnings: Vec::new(),
            },
            ClientResponse::ShellExecApprovalPrompt { id, .. } => Self {
                response_type: "shell_exec_approval_prompt".to_string(),
                id: id.clone(),
                status: None,
                assistant_message: None,
                tool_calls: Vec::new(),
                error_code: None,
                error_message: None,
                alive: None,
                warn_max_tool_rounds: false,
                filter_warnings: Vec::new(),
                filter_stderr: Vec::new(),
                tool_warnings: Vec::new(),
            },
            ClientResponse::Cancelled { id, reason, .. } => Self {
                response_type: "cancelled".to_string(),
                id: id.clone(),
                status: None,
                assistant_message: None,
                tool_calls: Vec::new(),
                error_code: None,
                error_message: reason.clone(),
                alive: None,
                warn_max_tool_rounds: false,
                filter_warnings: Vec::new(),
                filter_stderr: Vec::new(),
                tool_warnings: Vec::new(),
            },
            ClientResponse::Progress { id, phase, message } => Self {
                response_type: "progress".to_string(),
                id: id.clone(),
                status: Some(format!("{phase:?}").to_lowercase()),
                assistant_message: None,
                tool_calls: Vec::new(),
                error_code: None,
                error_message: message.clone(),
                alive: None,
                warn_max_tool_rounds: false,
                filter_warnings: Vec::new(),
                filter_stderr: Vec::new(),
                tool_warnings: Vec::new(),
            },
            ClientResponse::AssistantStreaming { id, delta } => Self {
                response_type: "assistant_streaming".to_string(),
                id: id.clone(),
                status: None,
                assistant_message: Some(aibe_protocol::ProtocolMessageOut {
                    role: "assistant".to_string(),
                    content: delta.clone(),
                }),
                tool_calls: Vec::new(),
                error_code: None,
                error_message: None,
                alive: None,
                warn_max_tool_rounds: false,
                filter_warnings: Vec::new(),
                filter_stderr: Vec::new(),
                tool_warnings: Vec::new(),
            },
            ClientResponse::RouteTurnResult { id, .. } => Self {
                response_type: "route_turn_result".to_string(),
                id: id.clone(),
                status: Some("ok".to_string()),
                assistant_message: None,
                tool_calls: Vec::new(),
                error_code: None,
                error_message: None,
                alive: None,
                warn_max_tool_rounds: false,
                filter_warnings: Vec::new(),
                filter_stderr: Vec::new(),
                tool_warnings: Vec::new(),
            },
        }
    }

    fn render_tsv(&self) -> String {
        let mut out = String::new();
        append_tsv_row(&mut out, "response_type", &self.response_type);
        append_tsv_row(&mut out, "id", &self.id);
        append_tsv_row(&mut out, "status", self.status.as_deref().unwrap_or(""));
        append_tsv_row(
            &mut out,
            "assistant_message.role",
            self.assistant_message
                .as_ref()
                .map(|m| m.role.as_str())
                .unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "assistant_message.content",
            self.assistant_message
                .as_ref()
                .map(|m| m.content.as_str())
                .unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "tool_calls.count",
            &self.tool_calls.len().to_string(),
        );
        append_tsv_row(
            &mut out,
            "error.code",
            self.error_code.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "error.message",
            self.error_message.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "alive",
            self.alive
                .map(|v| if v { "true" } else { "false" })
                .unwrap_or(""),
        );
        out
    }

    fn render_env(&self) -> String {
        let mut out = String::new();
        append_env_line(&mut out, "AI_RESPONSE_TYPE", &self.response_type);
        append_env_line(&mut out, "AI_RESPONSE_ID", &self.id);
        append_env_line(
            &mut out,
            "AI_RESPONSE_STATUS",
            self.status.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ASSISTANT_MESSAGE_ROLE",
            self.assistant_message
                .as_ref()
                .map(|m| m.role.as_str())
                .unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ASSISTANT_MESSAGE_CONTENT",
            self.assistant_message
                .as_ref()
                .map(|m| m.content.as_str())
                .unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_TOOL_CALLS_COUNT",
            &self.tool_calls.len().to_string(),
        );
        append_env_line(
            &mut out,
            "AI_ERROR_CODE",
            self.error_code.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ERROR_MESSAGE",
            self.error_message.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ALIVE",
            self.alive
                .map(|v| if v { "true" } else { "false" })
                .unwrap_or(""),
        );
        out
    }
}

pub fn format_tool_call_line(tc: &ExecutedToolCall) -> String {
    let status = match tc.status {
        ExecutedToolStatus::Ok => "ok",
        ExecutedToolStatus::Error => "error",
    };
    let args = truncate_bytes(&tc.arguments.to_string(), MAX_TOOL_OUTPUT_BYTES);
    let detail = match tc.status {
        ExecutedToolStatus::Ok => tc
            .output
            .as_deref()
            .map(|s| truncate_bytes(s, MAX_TOOL_OUTPUT_BYTES))
            .unwrap_or_default(),
        ExecutedToolStatus::Error => {
            let err = tc.error.as_deref().unwrap_or("");
            let msg = tc.message.as_deref().unwrap_or("");
            format!("{err}: {msg}")
        }
    };
    let mut line = format!(
        "ai: tool {} {} args={args} output={detail}",
        tc.name, status
    );
    if let Some(risk) = tc.risk_class {
        line.push_str(&format!(" risk={risk:?}"));
    }
    if let Some(approval) = tc.approval_state {
        line.push_str(&format!(" approval={approval:?}"));
    }
    if let Some(dry_run) = tc.dry_run {
        line.push_str(&format!(" dry_run={dry_run}"));
    }
    if let Some(decision) = tc.decision.as_deref() {
        line.push_str(&format!(" decision={decision}"));
    }
    if let Some(source) = tc.approval_source.as_deref() {
        line.push_str(&format!(" approval_source={source}"));
    }
    line
}

pub fn truncate_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated]", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::{ProtocolMessageOut, ToolApprovalState, ToolName, ToolRiskClass};
    use serde_json::json;

    #[test]
    fn startup_line_formats() {
        use crate::domain::{resolve_tools, ConfigToolsTokens};

        let r = resolve_tools(None, &ConfigToolsTokens::default()).unwrap();
        assert_eq!(format_tools_startup(&r.startup), "ai: tools enabled: none");

        let r = resolve_tools(Some("@read-only"), &ConfigToolsTokens::default()).unwrap();
        assert_eq!(
            format_tools_startup(&r.startup),
            "ai: tools enabled: read_file, list_dir, grep, git_diff, git_status (@read-only)"
        );

        let r = resolve_tools(Some("@full"), &ConfigToolsTokens::default()).unwrap();
        assert_eq!(
            format_tools_startup(&r.startup),
            "ai: tools enabled: read_file, list_dir, grep, git_diff, git_status (@full)"
        );

        let r = resolve_tools(Some("@exec"), &ConfigToolsTokens::default()).unwrap();
        assert_eq!(
            format_tools_startup(&r.startup),
            "warning: ai: tools enabled: shell_exec (@exec)"
        );
    }

    #[test]
    fn truncates_multibyte_safe() {
        let s = "あ".repeat(20_000);
        let out = truncate_bytes(&s, 10);
        assert!(out.ends_with("[truncated]"));
        assert!(out.len() < s.len());
    }

    #[test]
    fn pong_never_writes_stdout() {
        let out = render_response(&ClientResponse::Pong { id: "x".into() }, false);
        assert!(out.stdout.is_none());
        assert_eq!(out.stderr, vec!["ai: pong (x)"]);
    }

    #[test]
    fn empty_assistant_has_no_stdout() {
        let out = render_response(
            &ClientResponse::AgentTurnResult {
                id: "id".into(),
                status: AgentTurnStatus::Ok,
                assistant_message: ProtocolMessageOut {
                    role: "assistant".into(),
                    content: String::new(),
                },
                tool_calls: vec![],
            },
            false,
        );
        assert!(out.stdout.is_none());
    }

    #[test]
    fn max_tool_rounds_warning_on_stderr_only() {
        let out = render_response(
            &ClientResponse::AgentTurnResult {
                id: "id".into(),
                status: AgentTurnStatus::MaxToolRounds,
                assistant_message: ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "partial".into(),
                },
                tool_calls: vec![],
            },
            false,
        );
        assert_eq!(out.stdout.as_deref(), Some("partial"));
        assert_eq!(out.stderr.len(), 1);
        assert!(out.stderr[0].contains("max tool rounds"));
    }

    #[test]
    fn verbose_tools_on_stderr_not_stdout() {
        let huge = "x".repeat(MAX_TOOL_OUTPUT_BYTES + 500);
        let huge_len = huge.len();
        let out = render_response(
            &ClientResponse::AgentTurnResult {
                id: "id".into(),
                status: AgentTurnStatus::Ok,
                assistant_message: ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "final".into(),
                },
                tool_calls: vec![ExecutedToolCall::ok(
                    "c1".into(),
                    ToolName::read_file(),
                    json!({"path": "a"}),
                    huge,
                )],
            },
            true,
        );
        assert_eq!(out.stdout.as_deref(), Some("final"));
        assert_eq!(out.stderr.len(), 1);
        let line = &out.stderr[0];
        assert!(line.starts_with("ai: tool read_file ok"));
        assert!(line.contains("[truncated]"));
        assert!(line.len() < huge_len);
    }

    #[test]
    fn format_tool_call_line_truncates_output() {
        let huge = "y".repeat(MAX_TOOL_OUTPUT_BYTES + 100);
        let huge_len = huge.len();
        let line = format_tool_call_line(&ExecutedToolCall::ok(
            "c1".into(),
            ToolName::shell_exec(),
            json!({}),
            huge,
        ));
        assert!(line.contains("[truncated]"));
        assert!(line.len() < huge_len + 80);
    }

    #[test]
    fn format_tool_call_line_includes_audit_metadata_when_present() {
        let line = format_tool_call_line(
            &ExecutedToolCall::ok("c1".into(), ToolName::shell_exec(), json!({}), "ok".into())
                .with_audit(
                    ToolRiskClass::DangerousShell,
                    ToolApprovalState::ExplicitClientOptIn,
                    false,
                ),
        );
        assert!(line.contains("risk=DangerousShell"));
        assert!(line.contains("approval=ExplicitClientOptIn"));
        assert!(line.contains("dry_run=false"));
        assert!(line.contains("decision=executed"));
        assert!(line.contains("approval_source=client_tools_allowlist"));
    }
}
