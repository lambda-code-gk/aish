//! Smart Preprocessor observation log（append-only NDJSON）。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::domain::smart_preprocessor::{
    redact_for_evidence, SmartHeadScores, SmartPreprocessDecision,
};

#[derive(Debug, Clone)]
pub struct ObservationContext {
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub history_id: Option<String>,
    pub decision_path: String,
    pub route_turn_used: bool,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ObservationRecord {
    pub schema_version: &'static str,
    pub timestamp_ms: u64,
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub history_id: Option<String>,
    pub model_version: Option<String>,
    pub feature_hash_version: String,
    pub mode: String,
    pub intent: String,
    pub confidence_bps: u16,
    pub gate: String,
    pub head_scores: SmartHeadScores,
    pub decision_path: String,
    pub route_turn_used: bool,
    pub fallback_reason: Option<String>,
    pub signal_counts: usize,
    pub redaction_stats: RedactionStats,
}

#[derive(Debug, Serialize)]
pub struct RedactionStats {
    pub evidence_items: usize,
}

impl ObservationRecord {
    pub fn from_decision(decision: &SmartPreprocessDecision, ctx: ObservationContext) -> Self {
        Self {
            schema_version: "1",
            timestamp_ms: current_time_ms(),
            ai_session_id: ctx.ai_session_id.map(|value| limit_text(value, 80)),
            conversation_id: ctx.conversation_id.map(|value| limit_text(value, 80)),
            history_id: ctx.history_id.map(|value| limit_text(value, 80)),
            model_version: decision
                .model_version
                .clone()
                .map(|value| limit_text(value, 80)),
            feature_hash_version: decision.feature_hash_version.clone(),
            mode: decision.mode.as_str().to_string(),
            intent: decision.intent.as_str().to_string(),
            confidence_bps: decision.confidence_bps,
            gate: decision.gate.as_str().to_string(),
            head_scores: decision.head_scores.clone(),
            decision_path: limit_text(ctx.decision_path, 80),
            route_turn_used: ctx.route_turn_used,
            fallback_reason: ctx.fallback_reason.map(|value| limit_text(value, 120)),
            signal_counts: decision.signal_feature_count,
            redaction_stats: RedactionStats {
                evidence_items: decision.evidence.len(),
            },
        }
    }
}

pub fn default_observation_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/ai/smart_preprocessor/observation.jsonl")
}

pub fn resolve_session_error_summary(aish_session_dir: Option<&Path>) -> Option<String> {
    let dir = aish_session_dir?;
    let log_path = dir.join("session.jsonl");
    let tail = read_file_tail(&log_path, 8192)?;
    let lines: Vec<&str> = tail.lines().rev().take(5).collect();
    let joined = lines.into_iter().rev().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(redact_for_evidence(&joined, 400))
    }
}

fn read_file_tail(path: &Path, max_bytes: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes as u64);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

pub fn write_observation_record(
    path: &PathBuf,
    record: &ObservationRecord,
    max_bytes: usize,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let line = serde_json::to_string(record).map_err(|e| e.to_string())?;
    if line.len() + 1 > max_bytes {
        return Err(format!(
            "observation record exceeds max_bytes: {} + newline > {}",
            line.len(),
            max_bytes
        ));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    Ok(())
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn limit_text(value: String, max_len: usize) -> String {
    if value.len() <= max_len {
        return value;
    }
    if max_len == 0 {
        return String::new();
    }
    if max_len <= 3 {
        let mut end = max_len;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        return value[..end].to_string();
    }
    let mut end = max_len - 3;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::smart_preprocessor::{
        run_preprocessor, PreprocessConfig, PreprocessInput, RouteMetadataInput,
        SmartPreprocessMode,
    };

    #[test]
    fn observation_does_not_store_raw_user_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("obs.jsonl");
        let input = PreprocessInput {
            user_text: "エラー token=ghp_abc /home/user/secret".into(),
            command: Some("ask".into()),
            tty: true,
            new_conversation: true,
            conversation_id: None,
            memory_enabled: true,
            history_tail_summary: None,
            session_error_summary: None,
            cli_overrides: false,
            route_metadata: RouteMetadataInput::default(),
        };
        let mut cfg = PreprocessConfig::default();
        cfg.mode = SmartPreprocessMode::Shadow;
        let decision = run_preprocessor(input, &cfg);
        let record = ObservationRecord::from_decision(
            &decision,
            ObservationContext {
                ai_session_id: Some("sess".into()),
                conversation_id: None,
                history_id: None,
                decision_path: "shadow".into(),
                route_turn_used: true,
                fallback_reason: None,
            },
        );
        write_observation_record(&path, &record, 4096).expect("write");
        let content = std::fs::read_to_string(&path).expect("read");
        assert!(!content.contains("ghp_abc"));
        let value: serde_json::Value = serde_json::from_str(content.trim()).expect("valid json");
        assert!(value.get("head_scores").is_some());
    }

    #[test]
    fn session_summary_reads_tail_without_loading_entire_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("session.jsonl");
        let mut content = String::new();
        for i in 0..2000 {
            content.push_str(&format!("{{\"line\":{i}}}\n"));
        }
        content.push_str("{\"event\":\"error\",\"message\":\"tail marker\"}\n");
        fs::write(&log_path, content).expect("write");
        let summary = resolve_session_error_summary(Some(dir.path())).expect("summary");
        assert!(summary.contains("tail marker"));
    }
}
