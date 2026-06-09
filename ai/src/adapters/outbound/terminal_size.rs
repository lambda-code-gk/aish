//! 対話端末サイズの検出（I/O・環境変数）。

use std::io::{stderr, stdout, IsTerminal};
use std::os::unix::io::AsRawFd;

use crate::domain::TerminalSize;

/// 対話端末のサイズを検出する。非 TTY または取得不能時は `None`。
pub fn detect_terminal_size() -> Option<TerminalSize> {
    if !stderr().is_terminal() && !stdout().is_terminal() {
        return None;
    }
    winsize_from_fd(stderr().as_raw_fd())
        .or_else(|| winsize_from_fd(stdout().as_raw_fd()))
        .or_else(env_terminal_size)
}

fn winsize_from_fd(fd: std::os::fd::RawFd) -> Option<TerminalSize> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
        Some(TerminalSize {
            columns: ws.ws_col,
            rows: ws.ws_row,
        })
    } else {
        None
    }
}

fn env_terminal_size() -> Option<TerminalSize> {
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|v| *v > 0)?;
    let rows = std::env::var("LINES")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|v| *v > 0)?;
    Some(TerminalSize { columns, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_fallback_parses_columns_and_lines() {
        let prev_cols = std::env::var("COLUMNS").ok();
        let prev_lines = std::env::var("LINES").ok();
        std::env::set_var("COLUMNS", "100");
        std::env::set_var("LINES", "30");
        let size = env_terminal_size().expect("env size");
        assert_eq!(size.columns, 100);
        assert_eq!(size.rows, 30);
        restore_env("COLUMNS", prev_cols);
        restore_env("LINES", prev_lines);
    }

    fn restore_env(key: &str, value: Option<String>) {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
