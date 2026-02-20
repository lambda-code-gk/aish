//! EventHub: 1回の emit で全 sink へ EventRecord を配信する dispatcher
//!
//! sink 失敗時は他 sink への配信を継続し、警告を stderr に出す（best-effort）。

use crate::adapter::{HumanLogSink, TranscriptSink};
use crate::domain::event::{Event, EventRecord};
use crate::domain::SessionDir;
use crate::ports::outbound::{EnvResolver, EventRecordSink, FileSystem};
use chrono::Utc;
use std::sync::{Arc, Mutex};

/// 複数 sink へ順に配信する dispatcher
pub struct EventHub {
    sinks: Vec<Box<dyn EventRecordSink>>,
    seq: u64,
}

impl EventHub {
    pub fn new(sinks: Vec<Box<dyn EventRecordSink>>) -> Self {
        Self { sinks, seq: 0 }
    }

    /// 1イベントを ts/seq 付きで EventRecord にし、全 sink へ配信する。
    /// sink 失敗時は他 sink は継続し、警告のみ eprintln する。
    pub fn emit(&mut self, event: Event) {
        self.seq += 1;
        let ts = Utc::now().to_rfc3339();
        let rec = EventRecord {
            v: event.v,
            ts: ts.clone(),
            seq: self.seq,
            session_id: event.session_id,
            run_id: event.run_id,
            kind: event.kind,
            payload: event.payload,
        };

        for (i, sink) in self.sinks.iter_mut().enumerate() {
            if let Err(e) = sink.emit(&rec) {
                eprintln!("[event_hub] sink #{} emit failed: {}", i, e);
            }
        }
    }
}

/// 共有ハンドル（ai / aish の usecase や adapter から emit しやすくする）
#[derive(Clone)]
pub struct EventHubHandle(pub std::sync::Arc<Mutex<EventHub>>);

impl EventHubHandle {
    /// ロックして hub.emit を呼ぶ
    pub fn emit(&self, event: Event) {
        if let Ok(mut hub) = self.0.lock() {
            hub.emit(event);
        } else {
            eprintln!("[event_hub] lock poisoned");
        }
    }
}

/// EventHub を生成する。
/// `human_log`: true のときのみ HumanLogSink（[event] を stderr に出す）を追加。デフォルトでは出さない。
/// TranscriptSink は常に追加（session ありなら session_dir 配下、なしなら state_dir/transcript.jsonl）。
/// 生成失敗時は警告のみ出して継続（best-effort）。ai / aish の両方から利用可能。
pub fn build_event_hub(
    session_dir: Option<&SessionDir>,
    env: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
    human_log: bool,
) -> EventHubHandle {
    let mut sinks: Vec<Box<dyn EventRecordSink>> = vec![];
    if human_log {
        sinks.push(Box::new(HumanLogSink::new()));
    }
    match TranscriptSink::new(session_dir, env, fs) {
        Ok(transcript) => sinks.push(Box::new(transcript)),
        Err(e) => eprintln!("[event_hub] transcript sink init failed (continuing without): {}", e),
    }
    let hub = EventHub::new(sinks);
    EventHubHandle(Arc::new(Mutex::new(hub)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event::{RunId, SessionId};
    use std::sync::{Arc, Mutex};

    /// テスト用: 受け取った EventRecord を蓄積する sink（Arc で共有、Send 対応）
    struct CollectSink(Arc<Mutex<Vec<EventRecord>>>);

    impl EventRecordSink for CollectSink {
        fn emit(&mut self, rec: &EventRecord) -> anyhow::Result<()> {
            self.0.lock().unwrap().push(rec.clone());
            Ok(())
        }
    }

    #[test]
    fn event_to_record_has_ts_and_seq() {
        let out = Arc::new(Mutex::new(Vec::new()));
        let sink = CollectSink(Arc::clone(&out));
        let mut hub = EventHub::new(vec![Box::new(sink)]);

        let ev = Event {
            v: 1,
            session_id: SessionId::new("s1"),
            run_id: RunId::new("r1"),
            kind: "run.started".to_string(),
            payload: serde_json::json!({}),
        };
        hub.emit(ev);

        let records = out.lock().unwrap();
        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.v, 1);
        assert_eq!(rec.seq, 1);
        assert!(!rec.ts.is_empty());
        assert!(rec.ts.contains('T')); // RFC3339
        assert_eq!(rec.session_id.0, "s1");
        assert_eq!(rec.run_id.0, "r1");
        assert_eq!(rec.kind, "run.started");
    }

    #[test]
    fn seq_increments_per_emit() {
        let out = Arc::new(Mutex::new(Vec::new()));
        let mut hub = EventHub::new(vec![Box::new(CollectSink(Arc::clone(&out)))]);

        for k in ["run.started", "run.completed"] {
            hub.emit(Event {
                v: 1,
                session_id: SessionId::new("s1"),
                run_id: RunId::new("r1"),
                kind: k.to_string(),
                payload: serde_json::Value::Null,
            });
        }

        let records = out.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].seq, 1);
        assert_eq!(records[1].seq, 2);
    }

    #[test]
    fn two_sinks_both_receive() {
        let out1 = Arc::new(Mutex::new(Vec::new()));
        let out2 = Arc::new(Mutex::new(Vec::new()));
        let mut hub = EventHub::new(vec![
            Box::new(CollectSink(Arc::clone(&out1))),
            Box::new(CollectSink(Arc::clone(&out2))),
        ]);

        hub.emit(Event {
            v: 1,
            session_id: SessionId::new("s1"),
            run_id: RunId::new("r1"),
            kind: "test".to_string(),
            payload: serde_json::json!(null),
        });

        assert_eq!(out1.lock().unwrap().len(), 1);
        assert_eq!(out2.lock().unwrap().len(), 1);
    }
}
