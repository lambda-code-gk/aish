//! manifest.jsonl のドメイン型（追記専用イベント）

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// message レコードの role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManifestRole {
    User,
    Assistant,
}

impl ManifestRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// message レコードの decision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManifestDecision {
    Allow,
    Deny,
    Mask,
}

/// message レコード（v1）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRecordV1 {
    pub v: u8,
    pub ts: String,
    pub id: String,
    pub role: ManifestRole,
    pub part_path: String,
    pub reviewed_path: String,
    pub decision: ManifestDecision,
    pub bytes: u64,
    pub hash64: String,
}

/// compaction レコード（v1）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionRecordV1 {
    pub v: u8,
    pub ts: String,
    pub from_id: String,
    pub to_id: String,
    pub summary_path: String,
    pub method: String,
    pub source_count: usize,
}

/// manifest の 1 行
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ManifestRecordV1 {
    Message(MessageRecordV1),
    Compaction(CompactionRecordV1),
}

impl ManifestRecordV1 {
    pub fn to_jsonl_line(&self) -> String {
        match serde_json::to_string(self) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Warning: failed to serialize manifest record: {}", e);
                "{}".to_string()
            }
        }
    }

    pub fn message(&self) -> Option<&MessageRecordV1> {
        match self {
            Self::Message(v) => Some(v),
            Self::Compaction(_) => None,
        }
    }

    pub fn compaction(&self) -> Option<&CompactionRecordV1> {
        match self {
            Self::Compaction(v) => Some(v),
            Self::Message(_) => None,
        }
    }
}

/// JSONL 全文を行単位で parse する。不正行は warning を出してスキップ。
pub fn parse_lines(s: &str) -> Vec<ManifestRecordV1> {
    let mut out = Vec::new();
    for (i, line) in s.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<ManifestRecordV1>(trimmed) {
            Ok(rec) => out.push(rec),
            Err(e) => eprintln!(
                "Warning: invalid manifest.jsonl line {} skipped: {}",
                i + 1,
                e
            ),
        }
    }
    out
}

/// 暗号強度を要求しない軽量ハッシュ（u64 hex）
pub fn hash64(content: &str) -> String {
    let mut h = DefaultHasher::new();
    content.hash(&mut h);
    format!("{:016x}", h.finish())
}

