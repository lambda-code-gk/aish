//! `ai chat` 用の Unicode 対応行入力（TTY は rustyline、非 TTY は plain read）。

use std::io::{self, IsTerminal, Write};

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

const PROMPT: &str = "ai> ";

pub enum ChatReadLineResult {
    Input(String),
    Eof,
}

/// TTY では Unicode 対応の行編集（矢印・Backspace 等）。pipe では 1 行 read。
pub fn read_chat_line() -> io::Result<ChatReadLineResult> {
    if io::stdin().is_terminal() {
        read_chat_line_interactive()
    } else {
        read_chat_line_plain()
    }
}

fn read_chat_line_interactive() -> io::Result<ChatReadLineResult> {
    let mut editor = DefaultEditor::new().map_err(|e| io::Error::other(e.to_string()))?;
    loop {
        match editor.readline(PROMPT) {
            Ok(line) => {
                if !line.is_empty() {
                    let _ = editor.add_history_entry(line.as_str());
                }
                return Ok(ChatReadLineResult::Input(line));
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C: 現在行を破棄して再プロンプト
                eprintln!();
                continue;
            }
            Err(ReadlineError::Eof) => return Ok(ChatReadLineResult::Eof),
            Err(e) => return Err(io::Error::other(e.to_string())),
        }
    }
}

fn read_chat_line_plain() -> io::Result<ChatReadLineResult> {
    eprint!("{PROMPT}");
    io::stderr().flush()?;
    let mut line = String::new();
    let n = io::stdin().read_line(&mut line)?;
    if n == 0 {
        return Ok(ChatReadLineResult::Eof);
    }
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(ChatReadLineResult::Input(line))
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_read_strips_newline_and_preserves_utf8() {
        // 非 TTY 経路のロジックを直接検証
        let mut line = "こんにちは\n".to_string();
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        assert_eq!(line, "こんにちは");
        assert!(line.is_ascii() || line.chars().all(|c| c != '\u{FFFD}'));
    }
}
