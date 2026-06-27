//! Smart Preprocessor observation NDJSON の read-only reader。

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::domain::SmartObservationLine;

#[derive(Debug)]
pub struct SmartObservationRead {
    pub total_records: usize,
    pub invalid_lines: usize,
    pub records: Vec<SmartObservationLine>,
}

#[derive(Debug, thiserror::Error)]
pub enum SmartObservationReadError {
    #[error("cannot open Smart Preprocessor observation log {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot read Smart Preprocessor observation log {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn read_smart_observation_log(
    path: &Path,
    limit: usize,
) -> Result<SmartObservationRead, SmartObservationReadError> {
    let file = File::open(path).map_err(|source| SmartObservationReadError::Open {
        path: path.into(),
        source,
    })?;
    if limit == 0 {
        return Ok(SmartObservationRead {
            total_records: 0,
            invalid_lines: 0,
            records: Vec::new(),
        });
    }
    let mut reader = BufReader::new(file);
    let mut tail = VecDeque::with_capacity(limit);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).map_err(|source| {
            SmartObservationReadError::Read {
                path: path.into(),
                source,
            }
        })? == 0
        {
            break;
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if tail.len() == limit {
            tail.pop_front();
        }
        tail.push_back(line.clone());
    }
    let total_records = tail.len();
    let mut invalid_lines = 0;
    let mut records = Vec::with_capacity(total_records);
    for line in tail {
        match serde_json::from_slice(&line) {
            Ok(record) => records.push(record),
            Err(_) => invalid_lines += 1,
        }
    }
    Ok(SmartObservationRead {
        total_records,
        invalid_lines,
        records,
    })
}

pub fn expand_observation_path(path: PathBuf) -> PathBuf {
    let Some(value) = path.to_str() else {
        return path;
    };
    if let Some(rest) = value.strip_prefix("~/") {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(rest)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reader_handles_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        fs::write(&path, "").unwrap();
        let read = read_smart_observation_log(&path, 1000).unwrap();
        assert_eq!(read.total_records, 0);
        assert_eq!(read.invalid_lines, 0);
    }

    #[test]
    fn reader_reports_missing_file_path() {
        let error = read_smart_observation_log(Path::new("/missing/observation.jsonl"), 20)
            .unwrap_err()
            .to_string();
        assert!(error.contains("/missing/observation.jsonl"));
        assert!(error.contains("cannot open"));
    }

    #[test]
    fn reader_counts_invalid_lines_and_keeps_valid_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("observations");
        fs::write(
            &path,
            "{\"timestamp_ms\":1,\"unknown\":true}\nnot-json\n{\"mode\":\"gate\"}\n",
        )
        .unwrap();
        let read = read_smart_observation_log(&path, 1000).unwrap();
        assert_eq!(read.total_records, 3);
        assert_eq!(read.invalid_lines, 1);
        assert_eq!(read.records.len(), 2);
        assert_eq!(read.records[0].timestamp_ms, Some(1));
    }

    #[test]
    fn reader_limit_keeps_last_non_empty_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("observations");
        fs::write(
            &path,
            "{\"timestamp_ms\":1}\n\n{\"timestamp_ms\":2}\n{\"timestamp_ms\":3}\n",
        )
        .unwrap();
        let read = read_smart_observation_log(&path, 2).unwrap();
        assert_eq!(read.records[0].timestamp_ms, Some(2));
        assert_eq!(read.records[1].timestamp_ms, Some(3));
    }
}
