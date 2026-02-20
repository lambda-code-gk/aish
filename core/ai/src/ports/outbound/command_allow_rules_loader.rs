//! 許可コマンドルール読み込みの Outbound ポート
//!
//! 解決済みパス（XDG / AISH_HOME に従う）から許可ルール一覧を返す。

use common::tool::CommandAllowRule;
use std::path::Path;

/// コマンド許可ルール設定ファイルのパスからルールを読み込む
pub trait CommandAllowRulesLoader: Send + Sync {
    fn load_rules(&self, path: &Path) -> Vec<CommandAllowRule>;
}
