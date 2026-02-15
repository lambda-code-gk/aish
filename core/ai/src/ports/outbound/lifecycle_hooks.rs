//! ライフサイクルフックの Outbound ポート
//!
//! usecase は決まったタイミングで on_event を呼ぶ。実装側で複数ハンドラを登録し順次実行する。

use crate::domain::LifecycleEvent;
use common::error::Error;

/// ライフサイクルイベントで登録済みの処理を実行する能力
pub trait LifecycleHooks: Send + Sync {
    fn on_event(&self, event: &LifecycleEvent) -> Result<(), Error>;
}
