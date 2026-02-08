//! タスク実行の Outbound ポート
//!
//! タスク名と引数で「存在すれば実行し終了コードを返す」責務。

use common::error::Error;

/// タスクを解決して実行する（存在する場合のみ）
///
/// - 戻り値: `Ok(Some(code))` 実行した終了コード, `Ok(None)` タスクなし, `Err` 実行時エラー
pub trait TaskRunner: Send + Sync {
    fn run_if_exists(&self, task_name: &str, args: &[String]) -> Result<Option<i32>, Error>;

    /// 利用可能なタスク名一覧を返す（補完用）。task.d のサブディレクトリ名と .sh のベース名。
    fn list_names(&self) -> Result<Vec<String>, Error>;
}
