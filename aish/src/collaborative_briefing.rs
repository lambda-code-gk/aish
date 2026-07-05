//! human shell 起動時の協調 handoff briefing（0055）。

use std::path::PathBuf;

/// human shell 起動直後に stderr へ出す briefing。human-shell 以外では `None`。
pub fn render_handoff_briefing_for_terminal() -> Option<String> {
    if std::env::var("AISH_CONTROL_MODE").as_deref() != Ok("human-shell") {
        return None;
    }
    let handoff_id = std::env::var("AISH_HANDOFF_ID").ok()?;
    if handoff_id.is_empty() {
        return None;
    }
    let root = std::env::var_os("AISH_HANDOFF_STORE_ROOT")?;
    let handoff = read_handoff_json(&PathBuf::from(root).join(&handoff_id).join("handoff.json"))?;
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

fn read_handoff_json(path: &std::path::Path) -> Option<HandoffBriefingSource> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    let mut source = HandoffBriefingSource {
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
        candidates: Vec::new(),
    };
    if let Some(execs) = value
        .get("requested_shell_execs")
        .and_then(|v| v.as_array())
    {
        for exec in execs {
            if let Some(command) = exec.get("command").and_then(|v| v.as_str()) {
                let args: Vec<&str> = exec
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|items| items.iter().filter_map(|arg| arg.as_str()).collect())
                    .unwrap_or_default();
                source.candidates.push(format_shell_command(command, &args));
            }
        }
    }
    if source.candidates.is_empty() {
        if let Some(parent) = path.parent() {
            source.candidates = read_candidate_commands(&parent.join("candidates.jsonl"));
        }
    }
    Some(source)
}

fn read_candidate_commands(path: &std::path::Path) -> Vec<String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return Vec::new(),
    };
    raw.lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
            value
                .get("command")
                .and_then(|command| command.as_str())
                .map(str::to_string)
        })
        .collect()
}

fn format_shell_command(command: &str, args: &[&str]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        format!("{command} {}", args.join(" "))
    }
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
    fn waiting_state_briefing_mentions_side_agent_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let handoff_dir = tmp.path().join("ho-test");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(
            handoff_dir.join("handoff.json"),
            r#"{"state":"SIDE_AGENT_WAITING_FOR_HUMAN","pending_human_request":"Run cargo test"}"#,
        )
        .unwrap();
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
        assert!(briefing.contains("side agent"));
        assert!(briefing.contains("side agent 再開"));
    }
}
