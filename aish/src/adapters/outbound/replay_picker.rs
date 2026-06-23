//! `aish replay pick` の fzf / 内蔵セレクタ（低レベル）。

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct PickerEntry {
    pub index: u32,
    pub line: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplayPickerError {
    #[error("replay pick requires a TTY on stdin, stdout, and stderr; use `aish replay list` and `aish replay show INDEX` instead")]
    NotTty,
    #[error("picker cancelled")]
    Cancelled,
    #[error("failed to run picker: {0}")]
    Failed(String),
}

pub fn require_interactive_tty() -> Result<(), ReplayPickerError> {
    require_tty()
}

pub fn pick_entry(entries: &[PickerEntry]) -> Result<u32, ReplayPickerError> {
    require_tty()?;
    if entries.is_empty() {
        return Err(ReplayPickerError::Failed("no entries".into()));
    }
    if fzf_available() {
        pick_with_fzf(entries)
    } else {
        pick_with_builtin(entries)
    }
}

fn require_tty() -> Result<(), ReplayPickerError> {
    if !is_tty(libc::STDIN_FILENO) || !is_tty(libc::STDOUT_FILENO) || !is_tty(libc::STDERR_FILENO) {
        return Err(ReplayPickerError::NotTty);
    }
    Ok(())
}

fn is_tty(fd: libc::c_int) -> bool {
    unsafe { libc::isatty(fd) != 0 }
}

pub fn fzf_available() -> bool {
    Command::new("fzf")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pick_with_fzf(entries: &[PickerEntry]) -> Result<u32, ReplayPickerError> {
    let mut child = Command::new("fzf")
        .arg("--delimiter=\t")
        .arg("--with-nth=2..")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ReplayPickerError::Failed(e.to_string()))?;

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        for entry in entries {
            writeln!(stdin, "{}\t{}", entry.index, entry.line)
                .map_err(|e| ReplayPickerError::Failed(e.to_string()))?;
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| ReplayPickerError::Failed(e.to_string()))?;
    if !output.status.success() {
        return Err(ReplayPickerError::Cancelled);
    }
    let selected = String::from_utf8_lossy(&output.stdout);
    parse_selected_index(&selected, entries)
}

fn pick_with_builtin(entries: &[PickerEntry]) -> Result<u32, ReplayPickerError> {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    for entry in entries {
        writeln!(err, "{}", entry.line).map_err(|e| ReplayPickerError::Failed(e.to_string()))?;
    }
    write!(err, "replay index> ").map_err(|e| ReplayPickerError::Failed(e.to_string()))?;
    err.flush()
        .map_err(|e| ReplayPickerError::Failed(e.to_string()))?;

    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| ReplayPickerError::Failed(e.to_string()))?;
    let line = line.trim();
    if line.is_empty() {
        return Err(ReplayPickerError::Cancelled);
    }
    let index: u32 = line
        .parse()
        .map_err(|_| ReplayPickerError::Failed(format!("invalid index: {line}")))?;
    if entries.iter().any(|e| e.index == index) {
        Ok(index)
    } else {
        Err(ReplayPickerError::Failed(format!(
            "index {index} is not in the list"
        )))
    }
}

fn parse_selected_index(selected: &str, entries: &[PickerEntry]) -> Result<u32, ReplayPickerError> {
    let line = selected.trim();
    if let Some((index_str, _rest)) = line.split_once('\t') {
        if let Ok(index) = index_str.trim().parse::<u32>() {
            if entries.iter().any(|e| e.index == index) {
                return Ok(index);
            }
        }
    }
    if let Some((_left, index)) = line.rsplit_once('\t') {
        if let Ok(index) = index.trim().parse::<u32>() {
            if entries.iter().any(|e| e.index == index) {
                return Ok(index);
            }
        }
    }
    if let Ok(index) = line.parse::<u32>() {
        if entries.iter().any(|e| e.index == index) {
            return Ok(index);
        }
    }
    Err(ReplayPickerError::Failed(
        "could not parse fzf selection".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_pick_rejects_non_tty() {
        let entries = vec![PickerEntry {
            index: 1,
            line: "1 echo".to_string(),
        }];
        let err = pick_entry(&entries).expect_err("non-tty");
        assert!(matches!(err, ReplayPickerError::NotTty));
    }

    #[test]
    fn replay_pick_prefers_fzf_when_available() {
        if fzf_available() {
            assert!(fzf_available());
        }
    }

    #[test]
    fn replay_pick_falls_back_to_builtin_selector() {
        let entries = vec![
            PickerEntry {
                index: 1,
                line: "1 echo".to_string(),
            },
            PickerEntry {
                index: 2,
                line: "2 ls".to_string(),
            },
        ];
        let line = format!("{}\t2", entries[1].line);
        let index = parse_selected_index(&line, &entries).expect("parse legacy");
        assert_eq!(index, 2);

        let indexed = format!("2\t{}", entries[1].line);
        let index = parse_selected_index(&indexed, &entries).expect("parse index-first");
        assert_eq!(index, 2);
    }
}
