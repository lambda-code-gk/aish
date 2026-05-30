//! ツール実行時のリクエスト単位コンテキスト。
//!
//! **カレントディレクトリ方針**（`docs/architecture.md`）:
//! 相対パス・`.` 付き設定ルートの解決は [`ToolExecutionContext::base_dir`] を使う。
//! aibe プロセスの [`std::env::current_dir`] を直接参照してはいけない。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::ClientCwd;
use crate::ports::outbound::ShellExecApprovalGate;

/// 1 回の `agent_turn` に紐づく実行コンテキスト。
#[derive(Clone)]
pub struct ToolExecutionContext {
    /// クライアント（例: `ai ask`）のカレントディレクトリ（絶対パス）。
    client_cwd: ClientCwd,
    turn_id: String,
    approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
}

impl std::fmt::Debug for ToolExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolExecutionContext")
            .field("client_cwd", &self.client_cwd)
            .field("turn_id", &self.turn_id)
            .field("approval_gate", &self.approval_gate.is_some())
            .finish()
    }
}

impl PartialEq for ToolExecutionContext {
    fn eq(&self, other: &Self) -> bool {
        self.client_cwd == other.client_cwd
            && self.turn_id == other.turn_id
            && self.approval_gate.is_some() == other.approval_gate.is_some()
    }
}

impl Eq for ToolExecutionContext {}

impl ToolExecutionContext {
    pub fn new(client_cwd: ClientCwd) -> Self {
        Self {
            client_cwd,
            turn_id: String::new(),
            approval_gate: None,
        }
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = turn_id.into();
        self
    }

    pub fn with_approval_gate(mut self, gate: Arc<dyn ShellExecApprovalGate>) -> Self {
        self.approval_gate = Some(gate);
        self
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn approval_gate(&self) -> Option<&Arc<dyn ShellExecApprovalGate>> {
        self.approval_gate.as_ref()
    }

    /// 相対パス解決・`allowed_roots` の `.` 展開の基準ディレクトリ。
    pub fn base_dir(&self) -> &Path {
        self.client_cwd.as_path()
    }

    /// 相対パスを `base_dir` 基準で絶対パスにする。既に絶対ならそのまま。
    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir().join(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_at(path: &str) -> ToolExecutionContext {
        ToolExecutionContext::new(ClientCwd::new(PathBuf::from(path)).expect("absolute cwd"))
    }

    #[test]
    fn resolve_path_relative_uses_base_dir() {
        let ctx = ctx_at("/tmp/client");
        assert_eq!(
            ctx.resolve_path(Path::new("a/b.txt")),
            PathBuf::from("/tmp/client/a/b.txt")
        );
    }

    #[test]
    fn resolve_path_absolute_unchanged() {
        let ctx = ctx_at("/tmp/client");
        assert_eq!(
            ctx.resolve_path(Path::new("/etc/hosts")),
            PathBuf::from("/etc/hosts")
        );
    }
}
