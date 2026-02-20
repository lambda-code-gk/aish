//! 人間向けログ Sink（EventRecord → stderr 等への要点のみ出力）
//!
//! 既存のロガー（tracing / log）に接続せず、MVP では stderr に整形して出力する。
//! payload の全量は出さず要点のみ（巨大化防止）。

use crate::domain::event::EventRecord;
use crate::ports::outbound::EventRecordSink;
use anyhow::Result;
use serde_json::Value;

const PAYLOAD_SUMMARY_MAX: usize = 400;

/// kind に応じて log level を決める（run.failed / tool.failed は warn）
fn is_warn_or_error(kind: &str) -> bool {
    kind == "run.failed"
        || kind == "tool.failed"
        || kind.ends_with(".failed")
        || kind.contains("error")
}

/// payload の要点だけを短い文字列にする（巨大化防止）
fn payload_summary(payload: &Value) -> String {
    if payload.is_null() || payload.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        return "{}".to_string();
    }
    let s = payload.to_string();
    if s.len() <= PAYLOAD_SUMMARY_MAX {
        return s;
    }
    let truncated = s.chars().take(PAYLOAD_SUMMARY_MAX).collect::<String>();
    format!("{}... (len={})", truncated, s.len())
}

/// 人間向けログ用 Sink（EventRecord を整形して stderr 等へ出力）
pub struct HumanLogSink;

impl HumanLogSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HumanLogSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventRecordSink for HumanLogSink {
    fn emit(&mut self, rec: &EventRecord) -> Result<()> {
        let summary = payload_summary(&rec.payload);
        let line = format!(
            "[event] {} #{} {} {}",
            rec.ts, rec.seq, rec.kind, summary
        );
        if is_warn_or_error(&rec.kind) {
            eprintln!("[event] warn: {} #{} {}", rec.ts, rec.seq, rec.kind);
            if !summary.is_empty() && summary != "{}" {
                eprintln!("  payload: {}", summary);
            }
        } else {
            eprintln!("{}", line);
        }
        Ok(())
    }
}
