//! Part ID生成: 固定長ASCII・辞書順＝時系列・同一ms内単調増加
//!
//! 形式: base62(0-9,A-Z,a-z) 8文字。値 = (ms since 2020-01-01)<<8 | seq(0..255)。辞書順＝数値順。
//! 既存の part_&lt;ID&gt;_user.txt / part_&lt;ID&gt;_assistant.txt のID部分のみこの形式に置き換える。
//!
//! 互換: 旧形式（base64url 8文字等）と混在してもファイル名順ソートは安定するが、
//! 辞書順が時系列に一致するのは新形式のみ。混在時は時系列順を保証しない。

use std::sync::atomic::{AtomicU64, Ordering};

static LAST_ID: AtomicU64 = AtomicU64::new(0);

const EPOCH_MS: u64 = 1577836800000; // 2020-01-01 00:00:00 UTC
const SEQ_BITS: u64 = 8;
const SEQ_MASK: u64 = (1 << SEQ_BITS) - 1; // 0..255
const BASE: u64 = 62;
const WIDTH: usize = 8;
const MAX_VAL: u64 = BASE.pow(WIDTH as u32) - 1;

/// 0-9, A-Z, a-z の順で辞書順＝数値順になるbase62
const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 新規Part IDを1つ生成する。辞書順ソートが生成順（時系列）になる固定長8文字ASCII。
pub fn generate_part_id() -> String {
    let ms = now_ms_u64();
    let ms_rel = ms.saturating_sub(EPOCH_MS);
    let base = (ms_rel << SEQ_BITS).min(MAX_VAL);

    loop {
        let prev = LAST_ID.load(Ordering::SeqCst);
        let next = if (prev >> SEQ_BITS) < ms_rel {
            base
        } else {
            let seq = (prev & SEQ_MASK) + 1;
            if seq > SEQ_MASK {
                continue; // 同一msでseq枯渇、次のmsまでリトライ
            }
            (prev + 1).min(MAX_VAL)
        };
        if LAST_ID.compare_exchange(prev, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            return to_base62(next);
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

fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_id_fixed_length_ascii() {
        let id = generate_part_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn part_id_lexicographic_monotonic_consecutive() {
        let ids: Vec<String> = (0..10).map(|_| generate_part_id()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "sort() must preserve generation order");
    }

    #[test]
    fn part_id_same_ms_monotonic() {
        let ids: Vec<String> = (0..50).map(|_| generate_part_id()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "rapid-fire IDs must be lexicographically monotonic");
    }
}
