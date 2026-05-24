//! ツール実行時のリクエスト単位コンテキスト。
//!
//! **カレントディレクトリ方針**（`docs/architecture.md`）:
//! 相対パス・`.` 付き設定ルートの解決は [`ToolExecutionContext::base_dir`] を使う。
//! aibe プロセスの [`std::env::current_dir`] を直接参照してはいけない。

use std::path::{Path, PathBuf};

use crate::domain::ClientCwd;

/// 1 回の `agent_turn` に紐づく実行コンテキスト。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionContext {
    /// クライアント（例: `ai ask`）のカレントディレクトリ（絶対パス）。
    client_cwd: ClientCwd,
}

impl ToolExecutionContext {
    pub fn new(client_cwd: ClientCwd) -> Self {
        Self { client_cwd }
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
