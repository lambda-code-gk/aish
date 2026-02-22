//! セッション配下のパス検証（パストラバーサル対策）
//!
//! manifest 由来の reviewed_path / summary_path を join する前に
//! basename 限定・フォーマット検証し、join 後に session_dir 配下であることを確認する。

use std::path::{Path, PathBuf};

/// reviewed ファイルを格納するセッション直下のサブディレクトリ名
pub const REVIEWED_DIR: &str = "reviewed";

/// 履歴送信開始位置（manifest の何件目から送るか）を記録するファイル名。内容は 1 行の非負整数（先頭=0）。
pub const HISTORY_SEND_FROM_FILENAME: &str = ".history_send_from";

const REVIEWED_PREFIX: &str = "reviewed_";
const REVIEWED_SUFFIX: &str = ".txt";
const SUMMARY_PREFIX: &str = "compaction_";
const SUMMARY_SUFFIX: &str = ".txt";

/// パス区切りを含まず単一成分か（`.` / `..` も禁止）
fn is_safe_basename_component(s: &str) -> bool {
    if s.is_empty() || s == "." || s == ".." {
        return false;
    }
    !s.contains('/') && !s.contains('\\')
}

/// reviewed ファイルの basename として許可するか。
/// 形式: `reviewed_` で始まり `.txt` で終わる単一成分。
pub fn is_safe_reviewed_basename(s: &str) -> bool {
    if !is_safe_basename_component(s) {
        return false;
    }
    let min_len = REVIEWED_PREFIX.len() + REVIEWED_SUFFIX.len();
    s.len() >= min_len && s.starts_with(REVIEWED_PREFIX) && s.ends_with(REVIEWED_SUFFIX)
}

/// manifest の reviewed_path として許可するか。
/// 形式: `reviewed/<basename>`（1階層のみ、`..` 禁止）。
pub fn is_safe_reviewed_path(s: &str) -> bool {
    if s.is_empty() || s.contains("..") || s.contains('\\') {
        return false;
    }
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 2 {
        return false;
    }
    parts[0] == REVIEWED_DIR && is_safe_reviewed_basename(parts[1])
}

/// compaction summary の basename として許可するか。
/// 形式: `compaction_` で始まり `.txt` で終わる単一成分。
pub fn is_safe_summary_basename(s: &str) -> bool {
    if !is_safe_basename_component(s) {
        return false;
    }
    let min_len = SUMMARY_PREFIX.len() + SUMMARY_SUFFIX.len();
    s.len() >= min_len && s.starts_with(SUMMARY_PREFIX) && s.ends_with(SUMMARY_SUFFIX)
}

/// `path` が実在し、正規化後に `session_dir` 配下にある場合にのみ `Some(正規化された path)` を返す。
/// ファイルが存在しない・配下でない場合は `None`。
pub fn resolve_under_session_dir(session_dir: &Path, path: &Path) -> Option<PathBuf> {
    let base = session_dir.canonicalize().ok()?;
    let resolved = path.canonicalize().ok()?;
    if resolved.strip_prefix(&base).is_ok() {
        Some(resolved)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_reviewed_basename_accepts_valid() {
        assert!(is_safe_reviewed_basename("reviewed_001_user.txt"));
        assert!(is_safe_reviewed_basename("reviewed_002_assistant.txt"));
    }

    #[test]
    fn test_is_safe_reviewed_basename_rejects_traversal() {
        assert!(!is_safe_reviewed_basename("../../etc/passwd"));
        assert!(!is_safe_reviewed_basename("reviewed_../secret.txt"));
        assert!(!is_safe_reviewed_basename("subdir/reviewed_x.txt"));
    }

    #[test]
    fn test_is_safe_reviewed_basename_rejects_bad_format() {
        assert!(!is_safe_reviewed_basename(""));
        assert!(!is_safe_reviewed_basename("."));
        assert!(!is_safe_reviewed_basename(".."));
        assert!(!is_safe_reviewed_basename("part_001.txt"));
        assert!(!is_safe_reviewed_basename("reviewed_001_user"));
    }

    #[test]
    fn test_is_safe_summary_basename_accepts_valid() {
        assert!(is_safe_summary_basename("compaction_001_002.txt"));
    }

    #[test]
    fn test_is_safe_summary_basename_rejects_traversal() {
        assert!(!is_safe_summary_basename("../../compaction_1_2.txt"));
    }

    #[test]
    fn test_is_safe_reviewed_path_accepts_subdir_form() {
        assert!(is_safe_reviewed_path("reviewed/reviewed_001_user.txt"));
        assert!(is_safe_reviewed_path("reviewed/reviewed_002_assistant.txt"));
    }

    #[test]
    fn test_is_safe_reviewed_path_rejects_traversal() {
        assert!(!is_safe_reviewed_path("reviewed/../../etc/passwd"));
        assert!(!is_safe_reviewed_path("../reviewed/reviewed_001_user.txt"));
    }

    #[test]
    fn test_resolve_under_session_dir_under_returns_some() {
        let dir = std::env::temp_dir().join(format!("safe_path_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("reviewed").join("reviewed_001_user.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "ok").unwrap();
        let resolved = resolve_under_session_dir(&dir, &file);
        assert!(resolved.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_under_session_dir_outside_returns_none() {
        let dir = std::env::temp_dir().join(format!("safe_path_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let outside = std::env::temp_dir().join("outside.txt");
        std::fs::write(&outside, "x").unwrap();
        let resolved = resolve_under_session_dir(&dir, &outside);
        assert!(resolved.is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(outside);
    }
}
