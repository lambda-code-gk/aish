//! セッション compaction（派生サマリ生成）Outbound ポート
//!
//! manifest のメッセージを選別・要約し、compaction レコードとサマリファイルを追加する。
//! wiring で実装を注入することで、deterministic / LLM要約 / embeddingクラスタ 等を差し替え可能にする。

use crate::domain::ManifestRecordV1;
use common::error::Error;
use common::ports::outbound::FileSystem;
use std::path::Path;

/// セッション履歴の compaction を行う能力
///
/// 呼び出し側が渡した manifest の records をもとに、必要ならサマリを生成し
/// manifest へ Compaction レコードを append する。
pub trait CompactionStrategy: Send + Sync {
    /// 閾値等を満たす場合に compaction を実行する（records は既に load 済みを想定）
    fn maybe_compact(
        &self,
        fs: &dyn FileSystem,
        session_dir: &Path,
        records: &[ManifestRecordV1],
    ) -> Result<(), Error>;
}
