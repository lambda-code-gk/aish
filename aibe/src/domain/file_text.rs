//! テキストファイルの検証・ハッシュ・改行種別（write tool 共通）。

use sha2::{Digest, Sha256};

/// ファイル内容の改行種別（設計 §8.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
    None,
    Mixed,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
            Self::None => "none",
            Self::Mixed => "mixed",
        }
    }
}

/// テキスト検証エラー（設計 §21 の語彙に対応）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTextError {
    InvalidUtf8,
    BinaryFileNotSupported,
    FileTooLarge,
    UnsupportedLineEndings,
}

impl FileTextError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidUtf8 => "invalid_utf8",
            Self::BinaryFileNotSupported => "binary_file_not_supported",
            Self::FileTooLarge => "file_too_large",
            Self::UnsupportedLineEndings => "unsupported_line_endings",
        }
    }
}

/// バイト列の SHA-256（小文字 hex、設計 §8.2）。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// UTF-8 テキストとして解釈可能か検証する（NUL 禁止）。
pub fn validate_utf8_bytes(bytes: &[u8]) -> Result<String, FileTextError> {
    if bytes.contains(&0) {
        return Err(FileTextError::BinaryFileNotSupported);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| FileTextError::InvalidUtf8)
}

/// ファイルサイズ上限（設計 §11.2）。
pub fn check_file_size(size: usize, max_bytes: usize) -> Result<(), FileTextError> {
    if size > max_bytes {
        Err(FileTextError::FileTooLarge)
    } else {
        Ok(())
    }
}

/// 改行種別を判定する（設計 §8.1）。
pub fn detect_line_ending(content: &str) -> LineEnding {
    if !content.contains('\n') && !content.contains('\r') {
        return LineEnding::None;
    }

    let mut has_bare_lf = false;
    let mut has_crlf = false;
    let mut has_bare_cr = false;

    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                has_crlf = true;
                i += 2;
                continue;
            }
            has_bare_cr = true;
        } else if bytes[i] == b'\n' {
            has_bare_lf = true;
        }
        i += 1;
    }

    if has_bare_cr || (has_crlf && has_bare_lf) {
        LineEnding::Mixed
    } else if has_crlf {
        LineEnding::Crlf
    } else if has_bare_lf {
        LineEnding::Lf
    } else {
        LineEnding::None
    }
}

/// ファイル末尾が改行で終わるか（raw bytes 基準、設計 §8.1）。
pub fn has_trailing_newline(bytes: &[u8]) -> bool {
    bytes.ends_with(b"\n")
}

/// write 時に mixed 改行を拒否する（Phase 7 で使用）。
pub fn reject_mixed_line_endings(content: &str) -> Result<LineEnding, FileTextError> {
    let kind = detect_line_ending(content);
    if kind == LineEnding::Mixed {
        Err(FileTextError::UnsupportedLineEndings)
    } else {
        Ok(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn line_endings() {
        assert_eq!(detect_line_ending("a\nb"), LineEnding::Lf);
        assert_eq!(detect_line_ending("a\r\nb"), LineEnding::Crlf);
        assert_eq!(detect_line_ending("plain"), LineEnding::None);
        assert_eq!(detect_line_ending("a\nb\r\nc\nd"), LineEnding::Mixed);
        assert_eq!(detect_line_ending("a\r\nb\nc"), LineEnding::Mixed);
    }

    #[test]
    fn text_validation() {
        assert!(validate_utf8_bytes(b"ok").is_ok());
        assert_eq!(
            validate_utf8_bytes(&[0xff, 0xfe]),
            Err(FileTextError::InvalidUtf8)
        );
        assert_eq!(
            validate_utf8_bytes(b"a\0b"),
            Err(FileTextError::BinaryFileNotSupported)
        );
    }

    #[test]
    fn file_size_limit() {
        assert!(check_file_size(10, 10).is_ok());
        assert_eq!(check_file_size(11, 10), Err(FileTextError::FileTooLarge));
    }

    #[test]
    fn trailing_newline() {
        assert!(has_trailing_newline(b"a\n"));
        assert!(has_trailing_newline(b"a\r\n"));
        assert!(!has_trailing_newline(b"a"));
        assert!(!has_trailing_newline(b""));
    }
}
