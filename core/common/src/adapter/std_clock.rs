//! 標準時刻実装（SystemTime を委譲）

use super::clock::Clock;
use std::time::{SystemTime, UNIX_EPOCH};

/// 標準ライブラリの SystemTime を使う Clock 実装
#[derive(Debug, Clone, Default)]
pub struct StdClock;

impl Clock for StdClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
