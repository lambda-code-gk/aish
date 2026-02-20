//! Transcript Sink（session_dir または global state_dir の transcript.jsonl へ append-only 記録）
//!
//! 1 イベント = 1 行 JSON（末尾 \n）。巨大 payload は preview 化して記録。
//! ファイルが N MB を超えたら transcript.1.jsonl にローテーションし、K 世代を保持する。

use crate::domain::event::EventRecord;
use crate::domain::SessionDir;
use crate::error::Error;
use crate::ports::outbound::{EnvResolver, EventRecordSink, FileSystem};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PREVIEW_MAX_LEN: usize = 2048;

/// ローテーション閾値（MB）。このサイズを超えたら transcript.1.jsonl に退避する。
const DEFAULT_MAX_SIZE_MB: u64 = 10;
/// 保持する世代数（transcript.jsonl, transcript.1.jsonl, ..., transcript.(K-1).jsonl）
const DEFAULT_KEEP_GENERATIONS: u32 = 3;

/// 巨大文字列を preview + len に置き換えた Value を返す（hash は MVP では省略）
fn sanitize_payload(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            if s.len() <= PREVIEW_MAX_LEN {
                Value::String(s.clone())
            } else {
                let preview: String = s.chars().take(PREVIEW_MAX_LEN).collect();
                serde_json::json!({ "preview": preview, "len": s.len() })
            }
        }
        Value::Array(arr) => {
            let out: Vec<Value> = arr.iter().map(sanitize_payload).collect();
            Value::Array(out)
        }
        Value::Object(obj) => {
            let out: serde_json::Map<String, Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), sanitize_payload(v)))
                .collect();
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// 永続化用の 1 行分（payload を sanitize したもの）
#[derive(Serialize)]
struct TranscriptLine<'a> {
    v: u32,
    ts: &'a str,
    seq: u64,
    session_id: &'a str,
    run_id: &'a str,
    kind: &'a str,
    #[serde(rename = "payload")]
    payload: Value,
}

/// session または global state_dir の transcript.jsonl へ append-only で書き出す Sink。N MB 超でローテーションする。
pub struct TranscriptSink {
    /// 配置ディレクトリ（transcript.jsonl の親）。セッション時は session_dir、無し時は state_dir。
    base_dir: PathBuf,
    writer: Option<std::io::BufWriter<std::fs::File>>,
    max_size_bytes: u64,
    keep_generations: u32,
}

impl TranscriptSink {
    /// セッションありなら session_dir 配下、なしなら env の state_dir に transcript.jsonl を開く。
    /// 親ディレクトリは create_dir_all する。デフォルトで 10 MB 超でローテーション、3 世代保持。
    pub fn new(
        session_dir: Option<&SessionDir>,
        env: Arc<dyn EnvResolver>,
        fs: Arc<dyn FileSystem>,
    ) -> Result<Self, Error> {
        Self::with_rotation(
            session_dir,
            env,
            fs,
            DEFAULT_MAX_SIZE_MB * 1024 * 1024,
            DEFAULT_KEEP_GENERATIONS,
        )
    }

    /// ローテーション閾値（bytes）と保持世代数を指定して作成する。
    pub fn with_rotation(
        session_dir: Option<&SessionDir>,
        env: Arc<dyn EnvResolver>,
        fs: Arc<dyn FileSystem>,
        max_size_bytes: u64,
        keep_generations: u32,
    ) -> Result<Self, Error> {
        let base_dir = match session_dir {
            Some(d) => d.as_ref().to_path_buf(),
            None => env
                .resolve_transcript_file_path()?
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        fs.create_dir_all(&base_dir)?;
        let path = base_dir.join("transcript.jsonl");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(Error::from)?;
        Ok(Self {
            base_dir,
            writer: Some(std::io::BufWriter::new(file)),
            max_size_bytes,
            keep_generations,
        })
    }

    fn current_path(&self) -> PathBuf {
        self.base_dir.join("transcript.jsonl")
    }

    /// 現在の transcript.jsonl が max_size を超えていればローテーションする。
    fn maybe_rotate(&mut self) -> Result<()> {
        if let Some(ref mut w) = self.writer {
            let _ = w.flush();
        }
        let path = self.current_path();
        let size = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => return Ok(()),
        };
        if size < self.max_size_bytes {
            return Ok(());
        }
        drop(self.writer.take());
        let k = self.keep_generations;
        let base = &self.base_dir;
        std::fs::remove_file(base.join(format!("transcript.{}.jsonl", k))).ok();
        for i in (1..k).rev() {
            let from = base.join(format!("transcript.{}.jsonl", i));
            let to = base.join(format!("transcript.{}.jsonl", i + 1));
            if from.exists() {
                std::fs::rename(&from, &to)?;
            }
        }
        std::fs::rename(&path, base.join("transcript.1.jsonl"))?;
        self.writer = Some(std::io::BufWriter::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| anyhow::anyhow!("open transcript {:?}: {}", path, e))?,
        ));
        Ok(())
    }

    fn writer_mut(&mut self) -> Result<&mut std::io::BufWriter<std::fs::File>> {
        if self.writer.is_none() {
            self.writer = Some(std::io::BufWriter::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.current_path())
                    .map_err(|e| anyhow::anyhow!("open transcript: {}", e))?,
            ));
        }
        Ok(self.writer.as_mut().unwrap())
    }

    fn should_flush(kind: &str) -> bool {
        kind == "run.completed" || kind == "run.failed"
    }
}

impl EventRecordSink for TranscriptSink {
    fn emit(&mut self, rec: &EventRecord) -> Result<()> {
        self.maybe_rotate()?;
        let w = self.writer_mut()?;
        let payload = sanitize_payload(&rec.payload);
        let line = TranscriptLine {
            v: rec.v,
            ts: &rec.ts,
            seq: rec.seq,
            session_id: &rec.session_id.0,
            run_id: &rec.run_id.0,
            kind: &rec.kind,
            payload,
        };
        serde_json::to_writer(&mut *w, &line)?;
        w.write_all(b"\n")?;
        if Self::should_flush(&rec.kind) {
            w.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{StdEnvResolver, StdFileSystem};
    use crate::domain::event::{RunId, SessionId};

    #[test]
    fn two_events_produce_two_lines_and_parseable_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let session_dir = SessionDir::new(dir.path().to_path_buf());
        let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let mut sink = TranscriptSink::new(Some(&session_dir), env, fs).unwrap();

        for (kind, seq) in [("run.started", 1u64), ("run.completed", 2u64)] {
            let rec = EventRecord {
                v: 1,
                ts: "2026-02-20T12:00:00Z".to_string(),
                seq,
                session_id: SessionId::new("s1"),
                run_id: RunId::new("r1"),
                kind: kind.to_string(),
                payload: serde_json::json!({ "note": "test" }),
            };
            sink.emit(&rec).unwrap();
        }
        drop(sink);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "expected 2 lines, got: {:?}", content);

        for line in &lines {
            let _: Value = serde_json::from_str(line).expect("each line must be valid JSON");
        }
    }

    #[test]
    fn large_string_in_payload_is_previewed() {
        let dir = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(dir.path().to_path_buf());
        let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let mut sink = TranscriptSink::new(Some(&session_dir), env, fs).unwrap();
        let big = "x".repeat(PREVIEW_MAX_LEN + 100);
        let rec = EventRecord {
            v: 1,
            ts: "2026-02-20T12:00:00Z".to_string(),
            seq: 1,
            session_id: SessionId::new("s1"),
            run_id: RunId::new("r1"),
            kind: "run.started".to_string(),
            payload: serde_json::json!({ "stdout": big }),
        };
        sink.emit(&rec).unwrap();
        drop(sink);

        let path = dir.path().join("transcript.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let payload = parsed.get("payload").unwrap();
        let stdout = payload.get("stdout").unwrap();
        let obj = stdout.as_object().expect("large string becomes object");
        assert!(obj.contains_key("preview"));
        assert_eq!(obj.get("len").and_then(Value::as_u64).unwrap(), (PREVIEW_MAX_LEN + 100) as u64);
    }

    #[test]
    fn session_dir_none_uses_global_path_from_env() {
        let dir = tempfile::tempdir().unwrap();
        let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let prev = std::env::var("AISH_HOME").ok();
        std::env::set_var("AISH_HOME", dir.path());
        let mut sink = TranscriptSink::new(None, env, fs).unwrap();
        if let Some(p) = prev {
            std::env::set_var("AISH_HOME", p);
        } else {
            std::env::remove_var("AISH_HOME");
        }
        let rec = EventRecord {
            v: 1,
            ts: "2026-02-20T12:00:00Z".to_string(),
            seq: 1,
            session_id: SessionId::new("global"),
            run_id: RunId::new("r1"),
            kind: "run.started".to_string(),
            payload: serde_json::json!({}),
        };
        sink.emit(&rec).unwrap();
        drop(sink);

        let path = dir.path().join("state").join("transcript.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
        let _: Value = serde_json::from_str(lines[0]).expect("valid JSON");
    }
}
