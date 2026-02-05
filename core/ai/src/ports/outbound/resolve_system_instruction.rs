//! 有効なシステムプロンプトを解決して 1 つの system instruction 文字列にする Outbound ポート

use common::error::Error;

/// -S 未指定時に、有効な sysq を結合した system instruction を返す
pub trait ResolveSystemInstruction: Send + Sync {
    fn resolve(&self) -> Result<Option<String>, Error>;
}
