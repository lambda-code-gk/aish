//! human shell 起動時の協調 handoff briefing（0055）。

use std::collections::HashSet;
use std::path::PathBuf;

use crate::human_shell::validate_handoff_id;

/// human shell 起動直後に stderr へ出す briefing。human-shell 以外では `None`。
pub fn render_handoff_briefing_for_terminal() -> Option<String> {
    if std::env::var("AISH_CONTROL_MODE").as_deref() != Ok("human-shell") {
        return None;
    }
    let handoff_id = std::env::var("AISH_HANDOFF_ID").ok()?;
    if handoff_id.is_empty() {
        return None;
    }
    validate_handoff_id(&handoff_id).ok()?;
    let root = std::env::var_os("AISH_HANDOFF_STORE_ROOT")?;
    let handoff = read_handoff_json(&PathBuf::from(root).join(&handoff_id))?;
    Some(format_handoff_briefing(&handoff_id, &handoff))
}

pub fn print_handoff_briefing_if_needed() {
    if let Some(briefing) = render_handoff_briefing_for_terminal() {
        for line in briefing.lines() {
            eprintln!("{line}");
        }
    }
}

#[derive(Debug, Default)]
struct HandoffBriefingSource {
    state: String,
    parent_request_summary: String,
    pending_human_request: String,
    candidates: Vec<String>,
}

fn read_handoff_json(handoff_dir: &std::path::Path) -> Option<HandoffBriefingSource> {
    let path = handoff_dir.join("handoff.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    let source = HandoffBriefingSource {
        state: value
            .get("state")
            .and_then(|state| state.as_str())
            .unwrap_or("HUMAN_ACTIVE")
            .to_string(),
        parent_request_summary: value
            .get("parent_request_summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        pending_human_request: value
            .get("pending_human_request")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        candidates: read_candidate_commands(&handoff_dir.join("candidates.jsonl")),
    };
    Some(source)
}

fn read_candidate_commands(path: &std::path::Path) -> Vec<String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    let mut seen = HashSet::new();
    raw.lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
            value
                .get("command")
                .and_then(|command| command.as_str())
                .filter(|command| seen.insert((*command).to_string()))
                .map(str::to_string)
        })
        .collect()
}

fn format_handoff_briefing(handoff_id: &str, source: &HandoffBriefingSource) -> String {
    let mut lines = vec!["ai: 協調作業 — 親エージェントが操作をあなたに移しました。".into()];
    if source.state == "SIDE_AGENT_WAITING_FOR_HUMAN" {
        lines[0] = "ai: 協調作業 — side agent があなたの操作を待っています。".into();
    }
    lines.push(String::new());
    if !source.parent_request_summary.is_empty() {
        lines.push(format!("目的: {}", source.parent_request_summary));
    }
    if !source.pending_human_request.is_empty() {
        lines.push(format!("依頼: {}", source.pending_human_request));
    }
    if !source.candidates.is_empty() {
        lines.push(String::new());
        lines.push("候補 (Alt+. / Alt+,):".into());
        for command in &source.candidates {
            lines.push(format!("  {command}"));
        }
    }
    lines.push(String::new());
    if source.state == "SIDE_AGENT_WAITING_FOR_HUMAN" {
        lines.push("side agent 再開: ai  または  ai <補足>".into());
    } else {
        lines.push("候補を確認・編集して実行してください（実行しない選択も可能）。".into());
        lines.push("質問・相談: human shell 内で `ai`".into());
    }
    lines.push("親へ戻る: Ctrl+D または exit".into());
    lines.push(format!("handoff: {handoff_id}"));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_candidates(path: &std::path::Path, commands: &[&str]) {
        let mut lines = String::new();
        for (index, command) in commands.iter().enumerate() {
            lines.push_str(&format!(
                r#"{{"id":"c{index}","command":"{command}","source":"PARENT_AGENT","target_handoff_id":"ho-test","created_at_ms":1}}"#
            ));
            lines.push('\n');
        }
        std::fs::write(path, lines).unwrap();
    }

    #[test]
    fn human_active_briefing_includes_goal_and_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let handoff_dir = tmp.path().join("ho-test");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(
            handoff_dir.join("handoff.json"),
            r#"{
                "state":"HUMAN_ACTIVE",
                "parent_request_summary":"cargo test を実行してテスト状況を確認したいです",
                "pending_human_request":"次のコマンドを確認し、必要なら実行してください: cargo test",
                "requested_shell_execs":[{"command":"cargo","args":["test"]}]
            }"#,
        )
        .unwrap();
        write_candidates(&handoff_dir.join("candidates.jsonl"), &["cargo test"]);
        unsafe {
            std::env::set_var("AISH_CONTROL_MODE", "human-shell");
            std::env::set_var("AISH_HANDOFF_ID", "ho-test");
            std::env::set_var("AISH_HANDOFF_STORE_ROOT", tmp.path());
        }
        let briefing = render_handoff_briefing_for_terminal().expect("briefing");
        unsafe {
            std::env::remove_var("AISH_CONTROL_MODE");
            std::env::remove_var("AISH_HANDOFF_ID");
            std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
        }
        assert!(briefing.contains("cargo test を実行"));
        assert!(briefing.contains("cargo test"));
        assert!(briefing.contains("Alt+."));
        assert!(briefing.contains("Ctrl+D"));
    }

    #[test]
    fn briefing_merges_side_agent_candidates_without_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let handoff_dir = tmp.path().join("ho-test");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(
            handoff_dir.join("handoff.json"),
            r#"{
                "state":"SIDE_AGENT_WAITING_FOR_HUMAN",
                "pending_human_request":"Run integration tests",
                "requested_shell_execs":[{"command":"cargo","args":["test"]}]
            }"#,
        )
        .unwrap();
        write_candidates(
            &handoff_dir.join("candidates.jsonl"),
            &["cargo test", "cargo test -p ai", "cargo test"],
        );
        unsafe {
            std::env::set_var("AISH_CONTROL_MODE", "human-shell");
            std::env::set_var("AISH_HANDOFF_ID", "ho-test");
            std::env::set_var("AISH_HANDOFF_STORE_ROOT", tmp.path());
        }
        let briefing = render_handoff_briefing_for_terminal().expect("briefing");
        unsafe {
            std::env::remove_var("AISH_CONTROL_MODE");
            std::env::remove_var("AISH_HANDOFF_ID");
            std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
        }
        assert!(briefing.contains("cargo test -p ai"));
        let candidate_lines: Vec<_> = briefing
            .split("候補 (Alt+. / Alt+,):")
            .nth(1)
            .unwrap_or("")
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("cargo"))
            .collect();
        assert_eq!(candidate_lines, vec!["cargo test", "cargo test -p ai"]);
    }

    #[test]
    fn briefing_uses_properly_quoted_candidate_command() {
        let tmp = tempfile::tempdir().unwrap();
        let handoff_dir = tmp.path().join("ho-quote");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(
            handoff_dir.join("handoff.json"),
            r#"{"state":"HUMAN_ACTIVE"}"#,
        )
        .unwrap();
        std::fs::write(
            handoff_dir.join("candidates.jsonl"),
            r#"{"id":"c1","command":"printf 'hello world' '$HOME'","source":"PARENT_AGENT","target_handoff_id":"ho-quote","created_at_ms":1}
"#,
        )
        .unwrap();
        unsafe {
            std::env::set_var("AISH_CONTROL_MODE", "human-shell");
            std::env::set_var("AISH_HANDOFF_ID", "ho-quote");
            std::env::set_var("AISH_HANDOFF_STORE_ROOT", tmp.path());
        }
        let briefing = render_handoff_briefing_for_terminal().expect("briefing");
        unsafe {
            std::env::remove_var("AISH_CONTROL_MODE");
            std::env::remove_var("AISH_HANDOFF_ID");
            std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
        }
        assert!(briefing.contains("printf 'hello world' '$HOME'"));
        assert!(!briefing.contains("printf hello world"));
    }

    #[test]
    fn invalid_handoff_id_skips_briefing() {
        unsafe {
            std::env::set_var("AISH_CONTROL_MODE", "human-shell");
            std::env::set_var("AISH_HANDOFF_ID", "../escape");
            std::env::set_var("AISH_HANDOFF_STORE_ROOT", "/tmp/aish-briefing-invalid");
        }
        assert!(render_handoff_briefing_for_terminal().is_none());
        unsafe {
            std::env::remove_var("AISH_CONTROL_MODE");
            std::env::remove_var("AISH_HANDOFF_ID");
            std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
        }
    }

    #[test]
    fn waiting_state_briefing_mentions_side_agent_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let handoff_dir = tmp.path().join("ho-waiting");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(
            handoff_dir.join("handoff.json"),
            r#"{"state":"SIDE_AGENT_WAITING_FOR_HUMAN","pending_human_request":"Run cargo test"}"#,
        )
        .unwrap();
        unsafe {
            std::env::set_var("AISH_CONTROL_MODE", "human-shell");
            std::env::set_var("AISH_HANDOFF_ID", "ho-waiting");
            std::env::set_var("AISH_HANDOFF_STORE_ROOT", tmp.path());
        }
        let briefing = render_handoff_briefing_for_terminal().expect("briefing");
        unsafe {
            std::env::remove_var("AISH_CONTROL_MODE");
            std::env::remove_var("AISH_HANDOFF_ID");
            std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
        }
        assert!(briefing.contains("side agent"));
        assert!(briefing.contains("side agent 再開"));
    }
}
