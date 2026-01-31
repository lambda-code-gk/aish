//! 時刻・乱数・ID 生成の種（アダプター）
//!
//! usecase はこの trait 経由で「現在時刻」を取得し、PartId 等の生成に使う。

/// 時刻取得の抽象
///
/// 実装は `common::adapter::StdClock` やテスト用の固定時刻など。
pub trait Clock: Send + Sync {
    /// 現在時刻をミリ秒（Unix epoch）で返す
    fn now_ms(&self) -> u64;
}
