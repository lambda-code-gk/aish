//! manifest.jsonl の読み書きユーティリティ

use crate::domain::{parse_lines, ManifestRecordV1};
use common::error::Error;
use common::ports::outbound::FileSystem;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn manifest_path(session_dir: &Path) -> PathBuf {
    session_dir.join("manifest.jsonl")
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

