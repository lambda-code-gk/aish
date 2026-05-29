//! セッション ID（2020 起点ミリ秒 → 12 桁小文字 hex）。

use std::time::{SystemTime, UNIX_EPOCH};

/// `2020-01-01T00:00:00Z` の Unix 時刻（秒）。
pub const EPOCH_2020_UNIX_SECS: u64 = 1_577_836_800;

/// 現在時刻から 2020 起点の経過ミリ秒（u64）。
pub fn ms_since_2020(now: SystemTime) -> u64 {
    let ms = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    ms.saturating_sub(EPOCH_2020_UNIX_SECS * 1000)
}

/// 経過ミリ秒を 12 桁小文字 hex にする（辞書順 = 時系列）。
pub fn format_session_id(ms: u64) -> String {
    format!("{ms:012x}")
}

/// `exists(ms)` が true の間、+1 ms して空き ID を返す。
pub fn next_available_ms<F>(mut ms: u64, mut exists: F) -> u64
where
    F: FnMut(u64) -> bool,
{
    while exists(ms) {
        ms = ms.saturating_add(1);
    }
    ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_twelve_lowercase_hex() {
        let id = format_session_id(0x0b2e8ba2e800);
        assert_eq!(id.len(), 12);
        assert!(id
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn lex_order_matches_numeric() {
        let a = format_session_id(100);
        let b = format_session_id(200);
        assert!(a < b);
    }

    #[test]
    fn next_available_bumps_on_collision() {
        let got = next_available_ms(100, |ms| ms == 100 || ms == 101);
        assert_eq!(got, 102);
    }
}
