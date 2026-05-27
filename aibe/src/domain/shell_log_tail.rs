//! aish ログ tail（エージェント turn への注入用）。

use aibe_protocol::SHELL_LOG_TAIL_MAX_BYTES;

/// 正規化済みシェルログ tail。空・空白のみは `None`、超過分は truncate 済み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLogTail(String);

impl ShellLogTail {
    pub const MAX_BYTES: usize = SHELL_LOG_TAIL_MAX_BYTES;

    /// wire 文字列を正規化。空・空白のみ → `None`。超過 → `MAX_BYTES` で truncate。
    pub fn from_wire_opt(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let s = if trimmed.len() > Self::MAX_BYTES {
            let end = trimmed.floor_char_boundary(Self::MAX_BYTES);
            trimmed[..end].to_string()
        } else {
            trimmed.to_string()
        };
        Some(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_wire_opt_accepts_normal_text() {
        let tail = ShellLogTail::from_wire_opt("line1\nline2").expect("some");
        assert_eq!(tail.as_str(), "line1\nline2");
    }

    #[test]
    fn from_wire_opt_empty_is_none() {
        assert!(ShellLogTail::from_wire_opt("").is_none());
    }

    #[test]
    fn from_wire_opt_whitespace_only_is_none() {
        assert!(ShellLogTail::from_wire_opt("  \t\n  ").is_none());
    }

    #[test]
    fn from_wire_opt_truncates_over_max_bytes() {
        let raw = "x".repeat(ShellLogTail::MAX_BYTES + 100);
        let tail = ShellLogTail::from_wire_opt(&raw).expect("some");
        assert_eq!(tail.as_str().len(), ShellLogTail::MAX_BYTES);
        assert_eq!(tail.as_str(), "x".repeat(ShellLogTail::MAX_BYTES));
    }
}
