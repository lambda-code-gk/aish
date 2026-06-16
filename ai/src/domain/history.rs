//! local history の index / payload ドメイン。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryIndexEntry {
    pub history_id: String,
    pub created_at_ms: u64,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_exec_approval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_plan: Option<String>,
    pub socket_path: String,
    pub request_kind: HistoryRecordKind,
    pub request_summary: HistorySummary,
    pub response_kind: HistoryRecordKind,
    pub response_summary: HistorySummary,
    pub status: HistoryRecordStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPayload {
    pub history_id: String,
    pub command: String,
    pub user_message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_messages: Vec<HistoryMessage>,
    /// smart feature 適用の redacted summary（replay 用 transcript とは別）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feature_summaries: Vec<HistoryMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_cwd: Option<String>,
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_exec_approval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_plan: Option<String>,
    pub socket_path: String,
    pub log_tail_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryReplayRequest {
    pub history_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryTurnInput {
    pub command: String,
    pub user_message: String,
    pub session_id: Option<String>,
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub preset: Option<String>,
    pub profile: Option<String>,
    pub shell_exec_approval: Option<String>,
    pub route_plan: Option<String>,
    pub socket_path: String,
    pub request_kind: HistoryRecordKind,
    pub request_summary: HistorySummary,
    pub response_kind: HistoryRecordKind,
    pub response_summary: HistorySummary,
    pub status: HistoryRecordStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryIndexFilter {
    pub session_id: Option<String>,
    pub command: Option<String>,
    pub status: Option<HistoryRecordStatus>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryIndexView {
    pub history_id: String,
    pub created_at_ms: u64,
    pub command: String,
    pub session_id: Option<String>,
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub preset: Option<String>,
    pub profile: Option<String>,
    pub shell_exec_approval: Option<String>,
    pub route_plan: Option<String>,
    pub socket_path: String,
    pub request_kind: HistoryRecordKind,
    pub request_summary: HistorySummary,
    pub response_kind: HistoryRecordKind,
    pub response_summary: HistorySummary,
    pub status: HistoryRecordStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryRecordKind {
    Ask,
    Retry,
    Rerun,
    Error,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryRecordStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorySummary {
    pub detail: String,
}

impl HistorySummary {
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl From<&HistoryIndexEntry> for HistoryIndexView {
    fn from(value: &HistoryIndexEntry) -> Self {
        Self {
            history_id: value.history_id.clone(),
            created_at_ms: value.created_at_ms,
            command: value.command.clone(),
            session_id: value.session_id.clone(),
            ai_session_id: value.ai_session_id.clone(),
            conversation_id: value.conversation_id.clone(),
            preset: value.preset.clone(),
            profile: value.profile.clone(),
            shell_exec_approval: value.shell_exec_approval.clone(),
            route_plan: value.route_plan.clone(),
            socket_path: value.socket_path.clone(),
            request_kind: value.request_kind.clone(),
            request_summary: value.request_summary.clone(),
            response_kind: value.response_kind.clone(),
            response_summary: value.response_summary.clone(),
            status: value.status.clone(),
        }
    }
}

impl HistoryIndexView {
    pub fn render_tsv(&self) -> String {
        let mut out = String::new();
        append_tsv_row(&mut out, "history_id", &self.history_id);
        append_tsv_row(&mut out, "created_at_ms", &self.created_at_ms.to_string());
        append_tsv_row(&mut out, "command", &self.command);
        append_tsv_row(
            &mut out,
            "session_id",
            self.session_id.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "conversation_id",
            self.conversation_id.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "ai_session_id",
            self.ai_session_id.as_deref().unwrap_or(""),
        );
        append_tsv_row(&mut out, "preset", self.preset.as_deref().unwrap_or(""));
        append_tsv_row(&mut out, "profile", self.profile.as_deref().unwrap_or(""));
        append_tsv_row(
            &mut out,
            "shell_exec_approval",
            self.shell_exec_approval.as_deref().unwrap_or(""),
        );
        append_tsv_row(
            &mut out,
            "route_plan",
            self.route_plan.as_deref().unwrap_or(""),
        );
        append_tsv_row(&mut out, "socket_path", &self.socket_path);
        append_tsv_row(&mut out, "request_kind", &self.request_kind.to_string());
        append_tsv_row(&mut out, "request_summary", &self.request_summary.detail);
        append_tsv_row(&mut out, "response_kind", &self.response_kind.to_string());
        append_tsv_row(&mut out, "response_summary", &self.response_summary.detail);
        append_tsv_row(&mut out, "status", &self.status.to_string());
        out
    }

    pub fn render_env(&self) -> String {
        let mut out = String::new();
        append_env_line(&mut out, "AI_HISTORY_ID", &self.history_id);
        append_env_line(
            &mut out,
            "AI_CREATED_AT_MS",
            &self.created_at_ms.to_string(),
        );
        append_env_line(&mut out, "AI_COMMAND", &self.command);
        append_env_line(
            &mut out,
            "AI_SESSION_ID",
            self.session_id.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_CONVERSATION_ID",
            self.conversation_id.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_AI_SESSION_ID",
            self.ai_session_id.as_deref().unwrap_or(""),
        );
        append_env_line(&mut out, "AI_PRESET", self.preset.as_deref().unwrap_or(""));
        append_env_line(
            &mut out,
            "AI_PROFILE",
            self.profile.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_SHELL_EXEC_APPROVAL",
            self.shell_exec_approval.as_deref().unwrap_or(""),
        );
        append_env_line(
            &mut out,
            "AI_ROUTE_PLAN",
            self.route_plan.as_deref().unwrap_or(""),
        );
        append_env_line(&mut out, "AI_SOCKET_PATH", &self.socket_path);
        append_env_line(&mut out, "AI_REQUEST_KIND", &self.request_kind.to_string());
        append_env_line(&mut out, "AI_REQUEST_SUMMARY", &self.request_summary.detail);
        append_env_line(
            &mut out,
            "AI_RESPONSE_KIND",
            &self.response_kind.to_string(),
        );
        append_env_line(
            &mut out,
            "AI_RESPONSE_SUMMARY",
            &self.response_summary.detail,
        );
        append_env_line(&mut out, "AI_STATUS", &self.status.to_string());
        out
    }
}

impl core::fmt::Display for HistoryRecordKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ask => write!(f, "ask"),
            Self::Retry => write!(f, "retry"),
            Self::Rerun => write!(f, "rerun"),
            Self::Error => write!(f, "error"),
            Self::Unknown(s) => f.write_str(s),
        }
    }
}

impl core::fmt::Display for HistoryRecordStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Error => write!(f, "error"),
        }
    }
}

use super::{append_env_line, append_tsv_row};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_and_env_hide_raw_payloads() {
        let entry = HistoryIndexEntry {
            history_id: "id".into(),
            created_at_ms: 1,
            command: "ask".into(),
            session_id: Some("sess".into()),
            ai_session_id: Some("ai-sess".into()),
            conversation_id: None,
            preset: Some("fast".into()),
            profile: Some("fast".into()),
            shell_exec_approval: Some("ask".into()),
            route_plan: Some("route".into()),
            socket_path: "/tmp/sock".into(),
            request_kind: HistoryRecordKind::Ask,
            request_summary: HistorySummary::new("user_message_len=12 shell_log_tail_len=4"),
            response_kind: HistoryRecordKind::Ask,
            response_summary: HistorySummary::new("assistant_message_len=8"),
            status: HistoryRecordStatus::Ok,
        };
        let view = HistoryIndexView::from(&entry);
        let tsv = view.render_tsv();
        assert!(tsv.contains("user_message_len=12"));
        assert!(tsv.contains("assistant_message_len=8"));
        let env = view.render_env();
        assert!(env.contains("AI_HISTORY_ID='id'"));

        let payload = HistoryPayload {
            history_id: "id".into(),
            command: "ask".into(),
            user_message: "hello".into(),
            request_messages: vec![],
            feature_summaries: vec![],
            shell_log_tail: None,
            client_cwd: None,
            tools: vec![],
            llm_profile: None,
            preset: None,
            session_id: Some("sess".into()),
            ai_session_id: Some("ai-sess".into()),
            conversation_id: Some("conv".into()),
            shell_exec_approval: Some("ask".into()),
            route_plan: Some("route".into()),
            socket_path: "/tmp/sock".into(),
            log_tail_bytes: 1,
        };
        assert_eq!(payload.conversation_id.as_deref(), Some("conv"));
        assert!(payload.request_messages.is_empty());
        assert!(payload.feature_summaries.is_empty());
    }

    #[test]
    fn history_payload_deserializes_without_feature_summaries() {
        let payload: HistoryPayload = serde_json::from_str(
            r#"{"history_id":"id","command":"ask","user_message":"hi","tools":[],"socket_path":"/tmp/s","log_tail_bytes":1,"request_messages":[{"role":"user","content":"hi"}]}"#,
        )
        .expect("deserialize");
        assert_eq!(payload.request_messages.len(), 1);
        assert!(payload.feature_summaries.is_empty());
    }
}
