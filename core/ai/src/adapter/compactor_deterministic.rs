//! deterministic compaction（正しさ非依存の最適化）

use crate::adapter::session_manifest;
use crate::domain::{ManifestRecordV1, ManifestRole};
use crate::domain::manifest::CompactionRecordV1;
use crate::ports::outbound::CompactionStrategy;
use common::error::Error;
use common::ports::outbound::{now_iso8601, FileSystem};
use common::safe_session_path::{is_safe_reviewed_path, resolve_under_session_dir};
use std::path::Path;

const DEFAULT_TRIGGER_MESSAGES: usize = 200;
const DEFAULT_CHUNK_MESSAGES: usize = 100;
const MAX_BULLET_CHARS: usize = 320;

/// 環境変数で閾値を切り、先頭行切り詰めでサマリを生成する compaction 実装
pub struct DeterministicCompactionStrategy;

impl CompactionStrategy for DeterministicCompactionStrategy {
    fn maybe_compact(
        &self,
        fs: &dyn FileSystem,
        session_dir: &Path,
        records: &[ManifestRecordV1],
    ) -> Result<(), Error> {
        if std::env::var("AISH_COMPACTION_ENABLE")
            .ok()
            .as_deref()
            != Some("1")
        {
            return Ok(());
        }

        let trigger = std::env::var("AISH_COMPACTION_TRIGGER_MESSAGES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_TRIGGER_MESSAGES);
        let chunk = std::env::var("AISH_COMPACTION_CHUNK_MESSAGES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_CHUNK_MESSAGES);

        let all_messages: Vec<_> = records.iter().filter_map(|r| r.message()).collect();
        if all_messages.len() <= trigger {
            return Ok(());
        }

        let last_to_id = records
            .iter()
            .filter_map(|r| r.compaction())
            .last()
            .map(|c| c.to_id.as_str());

        let candidates: Vec<_> = all_messages
            .into_iter()
            .filter(|m| last_to_id.map(|to| m.id.as_str() > to).unwrap_or(true))
            .collect();
        if candidates.len() <= trigger {
            return Ok(());
        }

        let selected: Vec<_> = candidates.into_iter().take(chunk).collect();
        if selected.is_empty() {
            return Ok(());
        }
        let from_id = selected.first().map(|m| m.id.clone()).unwrap_or_default();
        let to_id = selected.last().map(|m| m.id.clone()).unwrap_or_default();
        if from_id.is_empty() || to_id.is_empty() {
            return Ok(());
        }

        let summary_path = format!("compaction_{}_{}.txt", from_id, to_id);
        let summary_abs_path = session_dir.join(&summary_path);
        let summary = build_summary(fs, session_dir, &selected);
        fs.write(&summary_abs_path, &summary)?;

        let rec = ManifestRecordV1::Compaction(CompactionRecordV1 {
            v: 1,
            ts: now_iso8601(),
            from_id,
            to_id,
            summary_path,
            method: "deterministic".to_string(),
            source_count: selected.len(),
        });
        session_manifest::append(fs, session_dir, &rec)?;
        Ok(())
    }
}

fn build_summary(
    fs: &dyn FileSystem,
    session_dir: &Path,
    selected: &[&crate::domain::MessageRecordV1],
) -> String {
    let mut lines = Vec::new();
    lines.push("# Compaction summary (deterministic)".to_string());
    for msg in selected {
        let body = if is_safe_reviewed_path(&msg.reviewed_path) {
            let joined = session_dir.join(&msg.reviewed_path);
            resolve_under_session_dir(session_dir, &joined)
                .and_then(|safe_path| fs.read_to_string(&safe_path).ok())
        } else {
            None
        }
        .unwrap_or_else(|| "[unreadable reviewed content]".to_string());
        let first_line = body.lines().next().unwrap_or("").trim();
        let short = truncate_chars(first_line, MAX_BULLET_CHARS);
        let role = match msg.role {
            ManifestRole::User => "user",
            ManifestRole::Assistant => "assistant",
        };
        lines.push(format!("- [{}][{}] {}", msg.id, role, short));
    }
    lines.push(String::new());
    lines.push("(use history_get/search to retrieve full content)".to_string());
    lines.join("\n")
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in s.chars().take(max_chars) {
        out.push(ch);
    }
    if s.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_maybe_compact_appends_compaction_record_and_summary() {
        let _guard = env_lock().lock().expect("lock poisoned");
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().to_path_buf();
        let fs = StdFileSystem;
        let old_enable = std::env::var("AISH_COMPACTION_ENABLE").ok();
        let old_trigger = std::env::var("AISH_COMPACTION_TRIGGER_MESSAGES").ok();
        let old_chunk = std::env::var("AISH_COMPACTION_CHUNK_MESSAGES").ok();
        std::env::set_var("AISH_COMPACTION_ENABLE", "1");
        std::env::set_var("AISH_COMPACTION_TRIGGER_MESSAGES", "2");
        std::env::set_var("AISH_COMPACTION_CHUNK_MESSAGES", "2");

        let reviewed_dir = dir.join("reviewed");
        fs.create_dir_all(&reviewed_dir).unwrap();
        fs.write(&reviewed_dir.join("reviewed_001_user.txt"), "u1\nbody").unwrap();
        fs.write(&reviewed_dir.join("reviewed_002_assistant.txt"), "a2").unwrap();
        fs.write(&reviewed_dir.join("reviewed_003_user.txt"), "u3").unwrap();
        fs.write(
            &dir.join("manifest.jsonl"),
            "\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t1\",\"id\":\"001\",\"role\":\"user\",\"part_path\":\"part_001_user.txt\",\"reviewed_path\":\"reviewed/reviewed_001_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"aa\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t2\",\"id\":\"002\",\"role\":\"assistant\",\"part_path\":\"part_002_assistant.txt\",\"reviewed_path\":\"reviewed/reviewed_002_assistant.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"bb\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t3\",\"id\":\"003\",\"role\":\"user\",\"part_path\":\"part_003_user.txt\",\"reviewed_path\":\"reviewed/reviewed_003_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"cc\"}\n",
        )
        .unwrap();

        let records = session_manifest::load_all(&fs, &dir).unwrap();
        let strategy = DeterministicCompactionStrategy;
        strategy.maybe_compact(&fs, &dir, &records).unwrap();

        let body = fs.read_to_string(&dir.join("manifest.jsonl")).unwrap();
        let records = crate::domain::parse_lines(&body);
        let comp = records.iter().filter_map(|r| r.compaction()).last().unwrap();
        assert_eq!(comp.from_id, "001");
        assert_eq!(comp.to_id, "002");
        assert_eq!(comp.method, "deterministic");
        assert_eq!(comp.source_count, 2);
        let summary = fs.read_to_string(&dir.join(&comp.summary_path)).unwrap();
        assert!(summary.contains("[001][user]"));
        assert!(summary.contains("history_get/search"));

        if let Some(v) = old_enable {
            std::env::set_var("AISH_COMPACTION_ENABLE", v);
        } else {
            std::env::remove_var("AISH_COMPACTION_ENABLE");
        }
        if let Some(v) = old_trigger {
            std::env::set_var("AISH_COMPACTION_TRIGGER_MESSAGES", v);
        } else {
            std::env::remove_var("AISH_COMPACTION_TRIGGER_MESSAGES");
        }
        if let Some(v) = old_chunk {
            std::env::set_var("AISH_COMPACTION_CHUNK_MESSAGES", v);
        } else {
            std::env::remove_var("AISH_COMPACTION_CHUNK_MESSAGES");
        }
    }
}

