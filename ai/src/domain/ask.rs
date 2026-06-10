//! ユーザー質問の入力モデル。

use std::path::PathBuf;

use aibe_protocol::ToolName;
use thiserror::Error;

use super::RequestContextInput;

/// CLI / ユースケースが収集する入力（cwd は取得時点では未検証）。
#[derive(Debug, Clone)]
pub struct AskInput {
    pub user_message: String,
    pub shell_log_tail: Option<String>,
    /// `ai` プロセスのカレントディレクトリ。
    pub client_cwd: Option<PathBuf>,
    /// 展開・検証済みツール名。
    pub tools: Vec<ToolName>,
    pub llm_profile: Option<String>,
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
}

/// aibe へ送る `agent_turn` 用ペイロード。
#[derive(Debug, Clone)]
pub struct AskRequest {
    pub user_message: String,
    pub shell_log_tail: Option<String>,
    /// ツール有効時は必須。無効時は未送信でよい。
    pub client_cwd: Option<PathBuf>,
    pub tools: Vec<ToolName>,
    pub llm_profile: Option<String>,
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
    /// turn 解決済みの `RequestContext`（`aibe_client` は serialize のみ行う）。
    pub request_context: RequestContextInput,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AskRequestError {
    #[error("client cwd is unavailable; cannot run tools from this directory")]
    MissingClientCwd,
}

impl AskInput {
    /// ツール有効時は絶対 cwd が必須。aibe 送信直前に変換する。
    pub fn into_request(self) -> Result<AskRequest, AskRequestError> {
        if self.tools.is_empty() {
            return Ok(AskRequest {
                user_message: self.user_message,
                shell_log_tail: self.shell_log_tail,
                client_cwd: self.client_cwd,
                tools: self.tools,
                llm_profile: self.llm_profile,
                ai_session_id: self.ai_session_id,
                conversation_id: self.conversation_id,
                request_context: RequestContextInput::default(),
            });
        }

        let client_cwd = self.client_cwd.ok_or(AskRequestError::MissingClientCwd)?;
        if !client_cwd.is_absolute() {
            return Err(AskRequestError::MissingClientCwd);
        }

        Ok(AskRequest {
            user_message: self.user_message,
            shell_log_tail: self.shell_log_tail,
            client_cwd: Some(client_cwd),
            tools: self.tools,
            llm_profile: self.llm_profile,
            ai_session_id: self.ai_session_id,
            conversation_id: self.conversation_id,
            request_context: RequestContextInput::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_request_allows_missing_cwd_without_tools() {
        let input = AskInput {
            user_message: "hi".into(),
            shell_log_tail: None,
            client_cwd: None,
            tools: vec![],
            llm_profile: None,
            ai_session_id: None,
            conversation_id: None,
        };
        assert!(input.into_request().is_ok());
    }

    #[test]
    fn into_request_requires_cwd_with_tools() {
        let input = AskInput {
            user_message: "hi".into(),
            shell_log_tail: None,
            client_cwd: None,
            tools: vec![ToolName::read_file()],
            llm_profile: None,
            ai_session_id: None,
            conversation_id: None,
        };
        assert_eq!(
            input.into_request().unwrap_err(),
            AskRequestError::MissingClientCwd
        );
    }

    #[test]
    fn into_request_preserves_tool_names() {
        let input = AskInput {
            user_message: "hi".into(),
            shell_log_tail: None,
            client_cwd: Some("/tmp".into()),
            tools: vec![ToolName::read_file(), ToolName::shell_exec()],
            llm_profile: None,
            ai_session_id: None,
            conversation_id: None,
        };
        let req = input.into_request().expect("request");
        assert_eq!(
            req.tools,
            vec![ToolName::read_file(), ToolName::shell_exec()]
        );
    }
}
