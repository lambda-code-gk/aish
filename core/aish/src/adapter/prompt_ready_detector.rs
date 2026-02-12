//! PTY 出力ストリームから PromptReady マーカーを検知する
//!
//! プロンプト末尾に埋め込まれた OSC シーケンス（ESC ] 999 ; aish-prompt-ready BEL）を
//! 検出し、入力受付状態になったタイミングで注入を行うために使う。

/// プロンプト入力受付状態を表すマーカー（OSC 999, BEL 終端）
/// .aishrc の PS1/PROMPT 末尾に埋め込む想定。
pub const AISH_PROMPT_READY_MARKER: &[u8] = b"\x1b]999;aish-prompt-ready\x07";

/// バイトストリームを渡し、マーカーが出現したら true を返す検出器
///
/// マーカーがチャンク境界で分割されて届く場合にも対応するため、
/// 直近のバイトをリングバッファで保持して検索する。
#[derive(Debug, Default)]
pub struct PromptReadyDetector {
    buf: Vec<u8>,
}

impl PromptReadyDetector {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(256),
        }
    }

    /// 受信したバイト列を渡す。マーカーを検出したら true を返す（1 回検出ごとに 1 回 true）。
    /// 検出後は内部バッファをリセットして次回に備える。
    pub fn feed(&mut self, chunk: &[u8]) -> bool {
        self.buf.extend_from_slice(chunk);
        const MAX_KEEP: usize = 256;
        if self.buf.len() > MAX_KEEP {
            self.buf.drain(0..self.buf.len() - MAX_KEEP);
        }
        if self.buf.windows(AISH_PROMPT_READY_MARKER.len()).any(|w| w == AISH_PROMPT_READY_MARKER) {
            self.buf.clear();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_finds_marker_in_single_chunk() {
        let mut d = PromptReadyDetector::new();
        assert!(!d.feed(b"prompt$ "));
        assert!(d.feed(b"\x1b]999;aish-prompt-ready\x07"));
    }

    #[test]
    fn test_detector_finds_marker_split_across_chunks() {
        let mut d = PromptReadyDetector::new();
        assert!(!d.feed(b"\x1b]999;aish-p"));
        // 2 チャンク目でマーカーが完成する
        assert!(d.feed(b"rompt-ready\x07"));
    }

    #[test]
    fn test_detector_resets_after_detection() {
        let mut d = PromptReadyDetector::new();
        assert!(d.feed(b"\x1b]999;aish-prompt-ready\x07"));
        // 検出後バッファは空。続けて別データを渡しても検出されない
        assert!(!d.feed(b"prompt$ "));
        // 再度マーカーを渡すと検出される
        assert!(d.feed(b"\x1b]999;aish-prompt-ready\x07"));
    }
}
