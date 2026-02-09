//! ストリーミングの「消費」実装（表示・保存の分離）
//!
//! StdoutSink: assistant text 表示、tool は "Running tool: <name>..." と args を表示
//! JsonlLogSink: AgentEvent を JSONL で追記
//! PartFileSink: 完了時に assistant テキストを part_*_assistant.txt に保存
//! StdEventSinkFactory: EventSinkFactory の標準実装（StdoutSink のみ）

use crate::ports::outbound::EventSinkFactory;
use common::error::Error;
use common::llm::events::LlmEvent;
use common::sink::{AgentEvent, EventSink};
use std::io::{self, Write};
use std::path::Path;

/// ANSI: ダークグレー（bright black）
const DARK_GREY: &str = "\x1b[90m";
/// ANSI: リセット
const RESET: &str = "\x1b[0m";

/// 標準出力へ表示（TextDelta と tool の args を表示）
pub struct StdoutSink {
    /// 不具合調査用: true のとき冗長なデバッグ行を stderr に出す
    verbose: bool,
}

impl StdoutSink {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }
}

impl Default for StdoutSink {
    fn default() -> Self {
        Self::new(false)
    }
}

/// EventSinkFactory の標準実装（StdoutSink のみを返す）
pub struct StdEventSinkFactory {
    verbose: bool,
}

impl StdEventSinkFactory {
    pub fn new(verbose: bool) -> Self {
        Self { verbose }
    }
}

impl EventSinkFactory for StdEventSinkFactory {
    fn create_sinks(&self) -> Vec<Box<dyn EventSink>> {
        vec![Box::new(StdoutSink::new(self.verbose))]
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
            AgentEvent::Llm(LlmEvent::ToolCallBegin {
                call_id,
                name,
                thought_signature,
            }) => {
                eprintln!("{}Running tool: {}...{}", DARK_GREY, name, RESET);
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] call_id={} thought_signature={:?}{}",
                        DARK_GREY,
                        call_id,
                        thought_signature,
                        RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::ToolCallArgsDelta { call_id, json_fragment }) => {
                if self.verbose {
                    let snippet = if json_fragment.len() > 120 {
                        format!("{}...", &json_fragment[..json_fragment.floor_char_boundary(120)])
                    } else {
                        json_fragment.clone()
                    };
                    eprintln!(
                        "{}  [verbose] ToolCallArgsDelta call_id={} fragment={}{}",
                        DARK_GREY, call_id, snippet, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::ToolCallEnd { call_id }) => {
                if self.verbose {
                    eprintln!("{}  [verbose] ToolCallEnd call_id={}{}", DARK_GREY, call_id, RESET);
                }
            }
            AgentEvent::Llm(LlmEvent::Completed { finish }) => {
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] LlmEvent::Completed finish={:?}{}",
                        DARK_GREY, finish, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::Failed { message }) => {
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] LlmEvent::Failed message={}{}",
                        DARK_GREY, message, RESET
                    );
                }
            }
            AgentEvent::ToolResult { name, args, result, .. } => {
                eprintln!("{}Tool {} args: {}{}", DARK_GREY, name, args, RESET);
                if self.verbose {
                    let result_str = result.to_string();
                    let snippet = if result_str.len() > 200 {
                        format!("{}...", &result_str[..result_str.floor_char_boundary(200)])
                    } else {
                        result_str
                    };
                    eprintln!(
                        "{}  [verbose] result={}{}",
                        DARK_GREY, snippet, RESET
                    );
                }
            }
            AgentEvent::ToolError { name, args, message, .. } => {
                eprintln!("{}Tool {} args: {} failed: {}{}", DARK_GREY, name, args, message, RESET);
            }
        }
        Ok(())
    }

    fn on_end(&mut self) -> Result<(), Error> {
        println!();
        io::stdout()
            .flush()
            .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
        Ok(())
    }
}

/// JSONL ログへ追記（デバッグ・永続化用）
#[allow(dead_code)] // 将来のログ永続化で使用
pub struct JsonlLogSink<W: Write + Send + Sync> {
    writer: W,
}

impl<W: Write + Send + Sync> JsonlLogSink<W> {
    #[allow(dead_code)]
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write + Send + Sync> EventSink for JsonlLogSink<W> {
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
        let mut sink = StdoutSink::new(false);
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
