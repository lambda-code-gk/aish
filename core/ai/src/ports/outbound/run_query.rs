//! クエリ実行の Outbound ポート（タスク未ヒット時の LLM 実行に利用）

use crate::domain::Query;
use common::domain::{ProviderName, SessionDir};
use common::error::Error;

/// クエリを LLM に送って実行する能力
///
/// TaskUseCase がタスク未存在時に利用する。AiUseCase が実装する。
pub trait RunQuery: Send + Sync {
    fn run_query(
        &self,
        session_dir: Option<SessionDir>,
        provider: Option<ProviderName>,
        query: &Query,
        system_instruction: Option<&str>,
    ) -> Result<i32, Error>;
}
