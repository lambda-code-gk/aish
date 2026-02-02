//! Part ID 生成 Outbound ポート
//!
//! usecase は IdGenerator を注入し、テストでは固定 ID を返す実装を渡せる。

use crate::domain::PartId;

/// PartId を生成する抽象（Outbound ポート）
pub trait IdGenerator: Send + Sync {
    fn next_id(&self) -> PartId;
}
