//! Part ID生成: 固定長ASCII・辞書順＝時系列・同一ms内単調増加
//!
//! 形式: base62(0-9,A-Z,a-z) 8文字。値 = (ms since 2020-01-01)<<8 | seq(0..255)。辞書順＝数値順。
//! 既存の part_&lt;ID&gt;_user.txt / part_&lt;ID&gt;_assistant.txt のID部分のみこの形式に置き換える。
//!
//! 互換: 旧形式（base64url 8文字等）と混在してもファイル名順ソートは安定するが、
//! 辞書順が時系列に一致するのは新形式のみ。混在時は時系列順を保証しない。
//!
//! usecase は IdGenerator を注入し、テストでは固定 ID を返す実装を渡せる。

use crate::adapter::Clock;
use crate::domain::PartId;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static LAST_ID: AtomicU64 = AtomicU64::new(0);

const EPOCH_MS: u64 = 1577836800000; // 2020-01-01 00:00:00 UTC
const SEQ_BITS: u64 = 8;
const SEQ_MASK: u64 = (1 << SEQ_BITS) - 1; // 0..255
const BASE: u64 = 62;
const WIDTH: usize = 8;
const MAX_VAL: u64 = BASE.pow(WIDTH as u32) - 1;

/// 0-9, A-Z, a-z の順で辞書順＝数値順になるbase62
const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// PartId を生成する抽象（テストでは固定 ID を返す実装を注入可能）
pub trait IdGenerator: Send + Sync {
    fn next_id(&self) -> PartId;
}

/// Clock + グローバルシーケンスで PartId を生成する標準実装
pub struct StdIdGenerator {
    clock: Arc<dyn Clock>,
}

impl StdIdGenerator {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }
}

impl IdGenerator for StdIdGenerator {
    fn next_id(&self) -> PartId {
        let ms = self.clock.now_ms();
        let ms_rel = ms.saturating_sub(EPOCH_MS);
        let base = (ms_rel << SEQ_BITS).min(MAX_VAL);

        loop {
            let prev = LAST_ID.load(Ordering::SeqCst);
            let next = if (prev >> SEQ_BITS) < ms_rel {
                base
            } else {
                let seq = (prev & SEQ_MASK) + 1;
                if seq > SEQ_MASK {
                    continue;
                }
                (prev + 1).min(MAX_VAL)
            };
            if LAST_ID.compare_exchange(prev, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                return PartId::new(to_base62(next));
            }
        }
    }
}

fn to_base62(mut n: u64) -> String {
    let mut buf = [0u8; WIDTH];
    for i in (0..WIDTH).rev() {
        buf[i] = ALPHABET[(n % BASE) as usize];
        n /= BASE;
    }
    std::str::from_utf8(&buf).unwrap().to_string()
}

impl PartId {
    /// 新規Part IDを1つ生成する（デフォルトの StdIdGenerator を使用）
    pub fn generate() -> Self {
        StdIdGenerator::new(Arc::new(crate::adapter::StdClock)).next_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_id_fixed_length_ascii() {
        let id = PartId::generate();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn part_id_lexicographic_monotonic_consecutive() {
        let ids: Vec<PartId> = (0..10).map(|_| PartId::generate()).collect();
        let mut sorted = ids.clone();
        sorted.sort_by(|a, b| (**a).cmp(&**b));
        assert_eq!(ids, sorted, "sort() must preserve generation order");
    }

    #[test]
    fn part_id_same_ms_monotonic() {
        let ids: Vec<PartId> = (0..50).map(|_| PartId::generate()).collect();
        let mut sorted = ids.clone();
        sorted.sort_by(|a, b| (**a).cmp(&**b));
        assert_eq!(ids, sorted, "rapid-fire IDs must be lexicographically monotonic");
    }
}
