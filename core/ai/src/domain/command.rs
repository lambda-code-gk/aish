//! ai コマンドの enum（Command Pattern）
//!
//! タスク実行 vs LLM対話の分岐を enum で明示する。

use crate::domain::{Query, TaskName};
use common::domain::{ModelName, ProviderName};

/// ai の実行モード
#[derive(Debug, Clone, PartialEq)]
pub enum AiCommand {
    /// ヘルプ表示
    Help,
    /// 現在有効なプロファイル一覧表示
    ListProfiles,
    /// 指定プロバイダで有効なツール一覧表示（未指定時は全ツール）
    ListTools {
        profile: Option<ProviderName>,
    },
    /// タスク実行（タスクが見つかった場合のみ、見つからなければ Query へ）
    Task {
        name: TaskName,
        args: Vec<String>,
        profile: Option<ProviderName>,
        model: Option<ModelName>,
        /// タスク未ヒット時に run_query へ委譲する際に渡すシステムプロンプト（-S/--system）
        system: Option<String>,
    },
    /// 保存された会話状態から再開（-c/--continue 指定時のみ）
    Resume {
        profile: Option<ProviderName>,
        model: Option<ModelName>,
        /// システムプロンプト（-S/--system で指定）
        system: Option<String>,
    },
    /// LLM クエリ（タスク未指定、またはタスクが見つからなかった場合）
    Query {
        profile: Option<ProviderName>,
        model: Option<ModelName>,
        query: Query,
        /// システムプロンプト（-S/--system で指定）
        system: Option<String>,
    },
}
