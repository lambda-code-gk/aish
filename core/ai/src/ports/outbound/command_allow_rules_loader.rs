//! 許可コマンドルール読み込みの Outbound ポート
//!
//! HomeDir から許可ルール一覧を返す。

use common::domain::HomeDir;
use common::tool::CommandAllowRule;

/// ホームディレクトリからコマンド許可ルールを読み込む
pub trait CommandAllowRulesLoader: Send + Sync {
    fn load_rules(&self, home_dir: &HomeDir) -> Vec<CommandAllowRule>;
}
