//! manifest.jsonl の読み書きユーティリティ

use crate::domain::{parse_lines, ManifestRecordV1};
use common::error::Error;
use common::ports::outbound::FileSystem;
use common::safe_session_path::HISTORY_SEND_FROM_FILENAME;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn manifest_path(session_dir: &Path) -> PathBuf {
    session_dir.join("manifest.jsonl")
}

fn send_from_path(session_dir: &Path) -> PathBuf {
    session_dir.join(HISTORY_SEND_FROM_FILENAME)
}

/// 履歴送信開始位置（manifest の何件目から送るか）を読む。ファイルが無い・不正時は 0（最先端）。
pub(crate) fn load_send_from_index(fs: &dyn FileSystem, session_dir: &Path) -> usize {
    let path = send_from_path(session_dir);
    if !fs.exists(&path) {
        return 0;
    }
    let s = match fs.read_to_string(&path) {
        Ok(x) => x,
        Err(_) => return 0,
    };
    let trimmed = s.trim();
    trimmed.parse::<usize>().unwrap_or(0)
}

/// 履歴送信開始位置を「最先端」（0）に設定する。呼び出し元は aish clear（直接ファイル書き）のため、テスト・将来用に残す。
#[allow(dead_code)]
pub(crate) fn write_send_from_front(fs: &dyn FileSystem, session_dir: &Path) -> Result<(), Error> {
    let path = send_from_path(session_dir);
    let content = "0\n";
    fs.write(&path, content)
        .map_err(|e| Error::io_msg(e.to_string()))
}

pub(crate) fn append(
    fs: &dyn FileSystem,
    session_dir: &Path,
    rec: &ManifestRecordV1,
) -> Result<(), Error> {
    let path = manifest_path(session_dir);
    let mut w = fs.open_append(&path)?;
    w.write_all(rec.to_jsonl_line().as_bytes())
        .map_err(|e| Error::io_msg(e.to_string()))?;
    w.write_all(b"\n")
        .map_err(|e| Error::io_msg(e.to_string()))?;
    Ok(())
}

pub(crate) fn load_all(
    fs: &dyn FileSystem,
    session_dir: &Path,
) -> Result<Vec<ManifestRecordV1>, Error> {
    let path = manifest_path(session_dir);
    if !fs.exists(&path) {
        return Ok(Vec::new());
    }
    let s = fs.read_to_string(&path)?;
    Ok(parse_lines(&s))
}

pub(crate) fn tail_message_records(
    records: &[ManifestRecordV1],
    n: usize,
) -> Vec<ManifestRecordV1> {
    if n == 0 {
        return Vec::new();
    }
    let mut out: Vec<ManifestRecordV1> = records
        .iter()
        .filter_map(|r| r.message().map(|_| r.clone()))
        .rev()
        .take(n)
        .collect();
    out.reverse();
    out
}

