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
    pub reason_codes: Vec<String>,
    pub failure_kind: Option<String>,
    pub context_needs: Vec<String>,
    pub tool_hints: Vec<String>,
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
            fallback_reason: normalize_fallback_reason(ctx.fallback_reason.as_deref()),
            signal_counts: decision.signal_feature_count,
            reason_codes: decision
                .reason_codes
                .iter()
                .map(|code| limit_text(code.clone(), 48))
                .take(16)
                .collect(),
            failure_kind: decision.failure_kind.map(|kind| kind.as_str().to_string()),
            context_needs: decision
                .context_needs
                .iter()
                .map(|need| need.as_str().to_string())
                .collect(),
            tool_hints: decision
                .tool_hints
                .iter()
                .map(|hint| hint.as_str().to_string())
                .collect(),
            redaction_stats: RedactionStats {
                evidence_items: decision.evidence.len(),
            },
        }
    }
}

/// observation 用に fallback_reason を固定コードへ正規化する（パス・生エラー文言を残さない）。
pub fn normalize_fallback_reason(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let normalized = if value.starts_with("route_turn_failed") {
        if value.contains("model_load_failed") {
            "route_turn_failed;model_load_failed"
        } else {
            "route_turn_failed"
        }
    } else if value.starts_with("model_load_failed") {
        "model_load_failed"
    } else {
        return None;
    };
    Some(normalized.to_string())
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
        build_hashed_features, hash_feature, run_preprocessor, PreprocessConfig, PreprocessInput,
        RouteMetadataInput, SmartPreprocessMode,
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
        assert!(value.get("reason_codes").is_none() || value["reason_codes"].is_array());
    }

    #[test]
    fn observation_persists_reason_codes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("obs.jsonl");
        let input = PreprocessInput {
            user_text: "hello".into(),
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
        assert!(!decision.reason_codes.is_empty());
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
        let value: serde_json::Value = serde_json::from_str(content.trim()).expect("json");
        let codes = value["reason_codes"].as_array().expect("reason_codes");
        assert!(!codes.is_empty());
        assert!(codes
            .iter()
            .all(|code| code.as_str().is_some_and(|s| !s.contains("ghp_"))));
    }

    #[test]
    fn observation_does_not_store_raw_paths_or_secrets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("obs.jsonl");
        let input = PreprocessInput {
            user_text: "secret token=ghp_abc /home/user/secret".into(),
            command: Some("ask".into()),
            tty: true,
            new_conversation: true,
            conversation_id: None,
            memory_enabled: true,
            history_tail_summary: Some("/home/user/project".into()),
            session_error_summary: Some("permission denied /etc/shadow".into()),
            cli_overrides: false,
            route_metadata: RouteMetadataInput::default(),
        };
        let mut cfg = PreprocessConfig::default();
        cfg.mode = SmartPreprocessMode::Shadow;
        let decision = run_preprocessor(input, &cfg);
        let record = ObservationRecord::from_decision(
            &decision,
            ObservationContext {
                ai_session_id: None,
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
        assert!(!content.contains("/home/user"));
        assert!(!content.contains("/etc/shadow"));
    }

    #[test]
    fn session_error_summary_uses_session_error_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("session.jsonl");
        fs::write(
            &log_path,
            r#"{"event":"error","message":"test failed: foo"}"#,
        )
        .expect("write");
        let summary = resolve_session_error_summary(Some(dir.path())).expect("summary");
        let input = PreprocessInput {
            user_text: "fix".into(),
            command: Some("ask".into()),
            tty: true,
            new_conversation: true,
            conversation_id: None,
            memory_enabled: true,
            history_tail_summary: None,
            session_error_summary: Some(summary),
            cli_overrides: false,
            route_metadata: RouteMetadataInput::default(),
        };
        let features = build_hashed_features(&input, 4096, 17);
        let marker = hash_feature("session_error_ngram:te", 4096, 17);
        assert!(features.iter().any(|feature| feature.index == marker));
    }

    #[test]
    fn observation_fallback_reason_omits_paths() {
        let input = PreprocessInput {
            user_text: "hello".into(),
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
                ai_session_id: None,
                conversation_id: None,
                history_id: None,
                decision_path: "shadow".into(),
                route_turn_used: true,
                fallback_reason: Some(
                    "model_load_failed:model file not found: /home/user/secret/model.json".into(),
                ),
            },
        );
        assert_eq!(record.fallback_reason.as_deref(), Some("model_load_failed"));

        let route_record = ObservationRecord::from_decision(
            &decision,
            ObservationContext {
                ai_session_id: None,
                conversation_id: None,
                history_id: None,
                decision_path: "text_only_fallback".into(),
                route_turn_used: false,
                fallback_reason: Some(
                    "route_turn_failed;model_load_failed:parse model /etc/foo failed".into(),
                ),
            },
        );
        assert_eq!(
            route_record.fallback_reason.as_deref(),
            Some("route_turn_failed;model_load_failed")
        );
    }

    #[test]
    fn normalize_fallback_reason_rejects_unknown_values() {
        assert_eq!(normalize_fallback_reason(None), None);
        assert_eq!(
            normalize_fallback_reason(Some("connect error: /tmp/foo")),
            None
        );
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
