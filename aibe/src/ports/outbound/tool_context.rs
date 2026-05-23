//! ツール実行時のリクエスト単位コンテキスト。
//!
//! **カレントディレクトリ方針**（`docs/architecture.md`）:
//! 相対パス・`.` 付き設定ルートの解決は [`ToolExecutionContext::base_dir`] を使う。
//! aibe プロセスの [`std::env::current_dir`] を直接参照してはいけない（後方互換のフォールバックは `base_dir` 内のみ）。

use std::path::{Path, PathBuf};

/// 1 回の `agent_turn` に紐づく実行コンテキスト。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolExecutionContext {
    /// クライアント（例: `ai ask`）のカレントディレクトリ（絶対パス）。`read_file` / `shell_exec` 等が参照する。
    pub client_cwd: Option<PathBuf>,
}

impl ToolExecutionContext {
    pub fn from_client_cwd(client_cwd: Option<PathBuf>) -> Self {
        Self { client_cwd }
    }

    /// 相対パス解決・`allowed_roots` の `.` 展開の基準ディレクトリ。
    ///
    /// 優先: `context.cwd`（クライアントの cwd）→ 未送信時のみ aibe プロセスの cwd。
    pub fn base_dir(&self) -> PathBuf {
        self.client_cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
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

    #[test]
    fn resolve_path_relative_uses_base_dir() {
        let ctx = ToolExecutionContext::from_client_cwd(Some(PathBuf::from("/tmp/client")));
        assert_eq!(
            ctx.resolve_path(Path::new("a/b.txt")),
            PathBuf::from("/tmp/client/a/b.txt")
        );
    }

    #[test]
    fn resolve_path_absolute_unchanged() {
        let ctx = ToolExecutionContext::from_client_cwd(Some(PathBuf::from("/tmp/client")));
        assert_eq!(
            ctx.resolve_path(Path::new("/etc/hosts")),
            PathBuf::from("/etc/hosts")
        );
    }
}
