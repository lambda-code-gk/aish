//! ストリーミングの「消費」実装（表示・保存の分離）
//!
//! StdoutSink: assistant text 表示、tool は "Running tool: <name>..." のみ
//! JsonlLogSink: AgentEvent を JSONL で追記
//! PartFileSink: 完了時に assistant テキストを part_*_assistant.txt に保存

use common::error::Error;
use common::llm::events::LlmEvent;
use common::sink::{AgentEvent, EventSink};
use std::io::{self, Write};
use std::path::Path;

/// 標準出力へ表示（TextDelta のみ表示、tool args は表示しない）
pub struct StdoutSink;

impl StdoutSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdoutSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for StdoutSink {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        match ev {
            AgentEvent::Llm(LlmEvent::TextDelta(s)) => {
                print!("{}", s);
                io::stdout()
                    .flush()
                    .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
            }
            AgentEvent::Llm(LlmEvent::ToolCallBegin { name, .. }) => {
                // ここでは名前のみ。引数は ToolResult/ToolError 時に表示する。
                eprintln!("\nRunning tool: {}...", name);
            }
            AgentEvent::ToolResult { name, args, .. } => {
                eprintln!("Tool {} args: {}", name, args);
            }
            AgentEvent::ToolError { name, args, message, .. } => {
                eprintln!("Tool {} args: {} failed: {}", name, args, message);
            }
            _ => {}
        }
        Ok(())
    }
}

/// JSONL ログへ追記（デバッグ・永続化用）
#[allow(dead_code)] // 将来のログ永続化で使用
pub struct JsonlLogSink<W: Write + Send> {
    writer: W,
}

impl<W: Write + Send> JsonlLogSink<W> {
    #[allow(dead_code)]
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write + Send> EventSink for JsonlLogSink<W> {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        // AgentEvent を簡易 JSON 化（LlmEvent 等は enum なので手動で line を書くか、serde で Serialize にする）
        let line = format!("{:?}\n", ev);
        self.writer
            .write_all(line.as_bytes())
            .map_err(|e| Error::io_msg(e.to_string()))?;
        Ok(())
    }
}

/// 完了時に assistant テキストを part_*_assistant.txt に保存する Sink
/// バッファに TextDelta を蓄積し、on_end でファイルに書き出す
#[allow(dead_code)] // 将来の part 保存分離で使用
pub struct PartFileSink {
    session_dir: std::path::PathBuf,
    part_filename: String,
    buffer: String,
}

#[allow(dead_code)] // テスト・将来の part 保存で使用
impl PartFileSink {
    pub fn new(session_dir: impl AsRef<Path>, part_filename: impl Into<String>) -> Self {
        Self {
            session_dir: session_dir.as_ref().to_path_buf(),
            part_filename: part_filename.into(),
            buffer: String::new(),
        }
    }

    pub fn assistant_text(&self) -> &str {
        &self.buffer
    }
}

impl EventSink for PartFileSink {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        if let AgentEvent::Llm(LlmEvent::TextDelta(s)) = ev {
            self.buffer.push_str(s);
        }
        Ok(())
    }

    fn on_end(&mut self) -> Result<(), Error> {
        if self.buffer.trim().is_empty() {
            return Ok(());
        }
        let path = self.session_dir.join(&self.part_filename);
        std::fs::write(&path, &self.buffer).map_err(|e| Error::io_msg(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::llm::events::LlmEvent;

    #[test]
    fn test_stdout_sink_text_delta() {
        let mut sink = StdoutSink::new();
        let ev = AgentEvent::Llm(LlmEvent::TextDelta("hello".to_string()));
        assert!(sink.on_event(&ev).is_ok());
    }

    #[test]
    fn test_part_file_sink_buffer() {
        let mut sink = PartFileSink::new("/tmp", "part_xxx_assistant.txt");
        sink.on_event(&AgentEvent::Llm(LlmEvent::TextDelta("a".to_string())))
            .unwrap();
        sink.on_event(&AgentEvent::Llm(LlmEvent::TextDelta("b".to_string())))
            .unwrap();
        assert_eq!(sink.assistant_text(), "ab");
    }
}
