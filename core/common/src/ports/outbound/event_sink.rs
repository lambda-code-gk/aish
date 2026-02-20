//! EventRecord 用 Sink ポート（Transcript / HumanLog）
//!
//! 1回の `emit` で複数 sink へ配信するための trait。
//! 既存の `sink::EventSink`（AgentEvent）とは別の契約。

use crate::domain::event::EventRecord;
use anyhow::Result;

/// EventRecord を1件受け取る Sink（&mut self: BufWriter / seq 等の内部状態を許容）
pub trait EventRecordSink: Send {
    fn emit(&mut self, rec: &EventRecord) -> Result<()>;
}
