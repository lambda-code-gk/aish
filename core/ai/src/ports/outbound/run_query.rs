//! クエリ実行の Outbound ポート（タスク未ヒット時の LLM 実行に利用）

use crate::domain::Query;
use common::domain::{ModelName, ProviderName, SessionDir};
use common::error::Error;
use common::event_hub::EventHubHandle;

/// クエリを LLM に送って実行する能力
///
/// TaskUseCase がタスク未存在時に利用する。AiUseCase が実装する。
/// `query` が None のときは「resume」意図（保存された続き用状態から再開）。Some のときは通常のクエリ送信。
/// `max_turns_override`: エージェントループの上限。None のときは既定値（16）。手動テストでは環境変数 AI_MAX_TURNS を渡す。
/// `tool_allowlist`: モードで指定した場合のツール許可リスト。None のときは全ツール。
/// `event_hub`: session_dir が決まった場所で生成した EventHub のハンドル。run.started / run.completed 等の emit に使う。
pub trait RunQuery: Send + Sync {
    fn run_query(
        &self,
        session_dir: Option<SessionDir>,
        provider: Option<ProviderName>,
        model: Option<ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        max_turns_override: Option<usize>,
        tool_allowlist: Option<&[String]>,
        event_hub: Option<EventHubHandle>,
    ) -> Result<i32, Error>;
}
