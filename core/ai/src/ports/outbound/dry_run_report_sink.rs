//! dry run 結果の出力先 Outbound ポート
//!
//! usecase が dry run の結果を組み立てたあと、この trait 経由で出力先に渡す。
//! どこにどう出力するかは adapter が実装する（例: stdout, ファイル, ログ）。

use crate::domain::DryRunInfo;
use common::error::Error;

/// dry run の結果を出力する Outbound ポート
pub trait DryRunReportSink: Send + Sync {
    /// 組み立て済みの dry run 結果を出力する
    fn report(&self, info: &DryRunInfo) -> Result<(), Error>;
}
