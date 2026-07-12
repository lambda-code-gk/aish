//! replayable shell log の読み込み（filesystem I/O は adapter 側）。

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use aish_replay::LogEvent;

/// Human Task Evidence 用 ranged scan の上限（8 MiB）。
pub const MAX_EVIDENCE_SCAN_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ReplaySourceError {
    #[error("replay log unreadable: {0}")]
    LogRead(String),
    #[error("replay log invalid line: {0}")]
    InvalidLine(String),
    #[error("replay log range invalid")]
    InvalidRange,
}

/// ranged reader の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangedReplayEvents {
    pub events: Vec<LogEvent>,
    pub truncated: bool,
}

pub fn load_replay_events(path: &Path) -> Result<Vec<LogEvent>, ReplaySourceError> {
    let file = File::open(path).map_err(|e| ReplaySourceError::LogRead(e.to_string()))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| ReplaySourceError::LogRead(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let event: LogEvent = serde_json::from_str(&line)
            .map_err(|e| ReplaySourceError::InvalidLine(e.to_string()))?;
        events.push(event);
    }
    Ok(events)
}

/// `start..end`（`end = None` は観測時点 EOF）の JSONL を bounded に読む。
///
/// ファイル全体をメモリへ載せず、最大 `max_bytes` だけを読む。
pub fn load_replay_events_in_range(
    path: &Path,
    start: u64,
    end: Option<u64>,
    max_bytes: usize,
) -> Result<RangedReplayEvents, ReplaySourceError> {
    let mut file =
        File::open(path).map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
    let file_len = file
        .metadata()
        .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?
        .len();

    let resolved_end = match end {
        Some(end) if end > file_len => return Err(ReplaySourceError::InvalidRange),
        Some(end) => end,
        None => file_len,
    };
    if start > resolved_end || start > file_len {
        return Err(ReplaySourceError::InvalidRange);
    }
    if start == resolved_end {
        // 空 range でも event 境界上でなければ invalid（行途中のゼロ長を Some(empty) にしない）
        ensure_event_boundary(&mut file, start, file_len)?;
        return Ok(RangedReplayEvents {
            events: Vec::new(),
            truncated: false,
        });
    }

    let span = resolved_end - start;
    let max_bytes_u64 = max_bytes as u64;
    let tail_scan = span > max_bytes_u64;
    let mut read_start = if tail_scan {
        resolved_end - max_bytes_u64
    } else {
        start
    };
    let mut truncated = tail_scan;

    if read_start > 0 {
        file.seek(SeekFrom::Start(read_start - 1))
            .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
        let mut prev = [0u8; 1];
        file.read_exact(&mut prev)
            .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
        if prev[0] != b'\n' {
            if tail_scan {
                file.seek(SeekFrom::Start(read_start))
                    .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
                let mut skipped = 0u64;
                let mut found_newline = false;
                while read_start + skipped < resolved_end {
                    let mut byte = [0u8; 1];
                    file.read_exact(&mut byte)
                        .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
                    skipped += 1;
                    if byte[0] == b'\n' {
                        found_newline = true;
                        break;
                    }
                }
                if !found_newline {
                    return Ok(RangedReplayEvents {
                        events: Vec::new(),
                        truncated: true,
                    });
                }
                read_start += skipped;
                truncated = true;
                if read_start >= resolved_end {
                    return Ok(RangedReplayEvents {
                        events: Vec::new(),
                        truncated: true,
                    });
                }
            } else {
                return Err(ReplaySourceError::InvalidRange);
            }
        }
    }

    let to_read = (resolved_end - read_start) as usize;
    file.seek(SeekFrom::Start(read_start))
        .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
    let mut buf = vec![0u8; to_read];
    if to_read > 0 {
        file.read_exact(&mut buf)
            .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
    }

    let (parse_buf, end_truncated) = if buf.is_empty() || buf.ends_with(b"\n") {
        (buf.as_slice(), false)
    } else if let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') {
        if tail_scan {
            (&buf[..=last_nl], true)
        } else if resolved_end == file_len {
            // EOF で最終行に改行が無い JSONL は許容する
            (buf.as_slice(), false)
        } else {
            return Err(ReplaySourceError::InvalidRange);
        }
    } else if tail_scan {
        return Ok(RangedReplayEvents {
            events: Vec::new(),
            truncated: true,
        });
    } else if resolved_end == file_len {
        (buf.as_slice(), false)
    } else {
        return Err(ReplaySourceError::InvalidRange);
    };

    let mut events = Vec::new();
    for line in parse_buf.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(line)
            .map_err(|_| ReplaySourceError::InvalidLine("line is not valid UTF-8".to_string()))?;
        if text.trim().is_empty() {
            continue;
        }
        let event: LogEvent = serde_json::from_str(text)
            .map_err(|e| ReplaySourceError::InvalidLine(e.to_string()))?;
        events.push(event);
    }

    Ok(RangedReplayEvents {
        events,
        truncated: truncated || end_truncated,
    })
}

fn ensure_event_boundary(
    file: &mut File,
    offset: u64,
    file_len: u64,
) -> Result<(), ReplaySourceError> {
    if offset == 0 || offset == file_len {
        return Ok(());
    }
    file.seek(SeekFrom::Start(offset - 1))
        .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
    let mut prev = [0u8; 1];
    file.read_exact(&mut prev)
        .map_err(|e| ReplaySourceError::LogRead(stable_io_message(&e)))?;
    if prev[0] == b'\n' {
        Ok(())
    } else {
        Err(ReplaySourceError::InvalidRange)
    }
}

fn stable_io_message(error: &std::io::Error) -> String {
    // OS 生メッセージや絶対 path を protocol へ載せないための安定文言。
    error.kind().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_log(contents: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("log.jsonl");
        let mut file = File::create(&path).expect("create");
        file.write_all(contents).expect("write");
        (dir, path)
    }

    fn event_line(command: &str, index: u32) -> String {
        format!(
            r#"{{"event":"command_start","command":"{command}","args":[],"command_index":{index},"started_at":"t{index}","kind":"shell"}}"#
        )
    }

    #[test]
    fn load_replay_events_in_range_reads_only_requested_bytes() {
        let line0 = event_line("before", 0);
        let line1 = event_line("inside", 1);
        let line2 = event_line("after", 2);
        let contents = format!("{line0}\n{line1}\n{line2}\n");
        let start = line0.len() as u64 + 1;
        let end = start + line1.len() as u64 + 1;
        let (_dir, path) = write_log(contents.as_bytes());
        let loaded =
            load_replay_events_in_range(&path, start, Some(end), MAX_EVIDENCE_SCAN_BYTES).unwrap();
        assert_eq!(loaded.events.len(), 1);
        assert!(!loaded.truncated);
        match &loaded.events[0] {
            LogEvent::CommandStart { command, .. } => assert_eq!(command, "inside"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn load_replay_events_in_range_rejects_start_beyond_eof() {
        let (_dir, path) = write_log(b"{}\n");
        let err = load_replay_events_in_range(&path, 10, Some(10), MAX_EVIDENCE_SCAN_BYTES)
            .expect_err("beyond eof");
        assert!(matches!(err, ReplaySourceError::InvalidRange));
    }

    #[test]
    fn load_replay_events_in_range_rejects_mid_line_start_without_tail_scan() {
        let line = event_line("cmd", 0);
        let contents = format!("{line}\n");
        let (_dir, path) = write_log(contents.as_bytes());
        let err = load_replay_events_in_range(&path, 3, Some(contents.len() as u64), 1024)
            .expect_err("mid-line");
        assert!(matches!(err, ReplaySourceError::InvalidRange));
    }

    #[test]
    fn load_replay_events_in_range_tail_scan_discards_partial_line() {
        let line0 = "x".repeat(16);
        let line1 = event_line("kept", 1);
        let contents = format!("{line0}\n{line1}\n");
        let (_dir, path) = write_log(contents.as_bytes());
        let end = contents.len() as u64;
        let max_bytes = line1.len() + 1 + 4;
        let loaded = load_replay_events_in_range(&path, 0, Some(end), max_bytes).unwrap();
        assert!(loaded.truncated);
        assert_eq!(loaded.events.len(), 1);
        match &loaded.events[0] {
            LogEvent::CommandStart { command, .. } => assert_eq!(command, "kept"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn load_replay_events_in_range_empty_range_ok_on_boundary() {
        let (_dir, path) = write_log(b"abc\n");
        // after newline (== EOF)
        let loaded = load_replay_events_in_range(&path, 4, Some(4), 1024).unwrap();
        assert!(loaded.events.is_empty());
        assert!(!loaded.truncated);
        // start of file
        let loaded0 = load_replay_events_in_range(&path, 0, Some(0), 1024).unwrap();
        assert!(loaded0.events.is_empty());
    }

    #[test]
    fn load_replay_events_in_range_empty_range_rejects_mid_line() {
        let (_dir, path) = write_log(b"abc\n");
        let err = load_replay_events_in_range(&path, 2, Some(2), 1024).expect_err("mid-line");
        assert!(matches!(err, ReplaySourceError::InvalidRange));
    }

    #[test]
    fn load_replay_events_in_range_skips_blank_lines() {
        let line = event_line("ok", 0);
        let contents = format!("\n{line}\n\n");
        let (_dir, path) = write_log(contents.as_bytes());
        let loaded = load_replay_events_in_range(
            &path,
            0,
            Some(contents.len() as u64),
            MAX_EVIDENCE_SCAN_BYTES,
        )
        .unwrap();
        assert_eq!(loaded.events.len(), 1);
    }

    #[test]
    fn load_replay_events_in_range_invalid_json_is_invalid_line() {
        let (_dir, path) = write_log(b"{not-json}\n");
        let err = load_replay_events_in_range(&path, 0, Some(11), 1024).expect_err("bad json");
        assert!(matches!(err, ReplaySourceError::InvalidLine(_)));
    }
}
