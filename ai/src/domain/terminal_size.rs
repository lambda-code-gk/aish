//! 対話端末サイズ（値オブジェクト）。

/// TTY 上の端末サイズ（列・行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}
