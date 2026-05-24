//! ユーザー質問の入力モデル。

use std::path::PathBuf;

use aibe::ToolName;
use thiserror::Error;

/// CLI / ユースケースが収集する入力（cwd は取得時点では未検証）。
#[derive(Debug, Clone)]
pub struct AskInput {
    pub user_message: String,
    pub shell_log_tail: Option<String>,
    /// `ai` プロセスのカレントディレクトリ。
    pub client_cwd: Option<PathBuf>,
    /// 展開・検証済みツール名。
    pub tools: Vec<ToolName>,
}

/// aibe へ送る `agent_turn` 用ペイロード。
#[derive(Debug, Clone)]
pub struct AskRequest {
    pub user_message: String,
    pub shell_log_tail: Option<String>,
    /// ツール有効時は必須。無効時は未送信でよい。
    pub client_cwd: Option<PathBuf>,
    pub tools: Vec<ToolName>,
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
        };
        let req = input.into_request().expect("request");
        assert_eq!(
            req.tools,
            vec![ToolName::read_file(), ToolName::shell_exec()]
        );
    }
}
