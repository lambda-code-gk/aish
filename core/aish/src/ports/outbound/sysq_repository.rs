//! システムプロンプト（sysq）の一覧・有効化・無効化 Outbound ポート

use common::error::Error;
use common::system_prompt::Scope;

/// sysq list の1行分
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysqListEntry {
    pub id: String,
    pub scope: Scope,
    pub enabled: bool,
    pub title: String,
}

/// システムプロンプトの一覧・有効/無効の永続化（Outbound ポート）
pub trait SysqRepository: Send + Sync {
    /// 全スコープのシステムプロンプト一覧と有効状態を返す
    fn list_entries(&self) -> Result<Vec<SysqListEntry>, Error>;

    /// 指定IDを有効化する（IDが存在するスコープの enabled に追加）
    fn enable(&self, ids: &[String]) -> Result<(), Error>;

    /// 指定IDを無効化する（該当スコープの enabled から削除）
    fn disable(&self, ids: &[String]) -> Result<(), Error>;
}
