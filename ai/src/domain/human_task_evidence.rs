//! Human Task Evidence の純粋変換（0061）。filesystem / env / process に触れない。

use aibe_protocol::{HumanTaskCommandEvidence, HumanTaskEvidence};
use aish_replay::{CommandKind, LogEvent, ReplayError};

/// Evidence に含める完了 Shell command の最大件数。
pub const MAX_EVIDENCE_COMMANDS: usize = 50;
/// 1 command 文字列の最大バイト数（ellipsis 込み）。
pub const MAX_EVIDENCE_COMMAND_BYTES: usize = 2 * 1024;
/// 保持する command 文字列の合計最大バイト数。
pub const MAX_EVIDENCE_TOTAL_COMMAND_BYTES: usize = 16 * 1024;

const ELLIPSIS: &str = "…";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HumanTaskEvidenceBuildError {
    #[error("replay span views failed")]
    InvalidLog,
}

/// parse 済み events から Human Task Evidence を構築する。
///
/// `replay_span_views` が `NoSpans` のときは空 Evidence（エラーではない）。
pub fn build_human_task_evidence(
    events: &[LogEvent],
    source_truncated: bool,
) -> Result<HumanTaskEvidence, HumanTaskEvidenceBuildError> {
    let views = match aish_replay::replay_span_views(events) {
        Ok(views) => views,
        Err(ReplayError::NoSpans) => {
            return Ok(HumanTaskEvidence {
                commands: Vec::new(),
                truncated: source_truncated,
            });
        }
        Err(_) => return Err(HumanTaskEvidenceBuildError::InvalidLog),
    };

    let mut selected: Vec<(String, Option<i32>)> = Vec::new();
    let mut dropped_old = false;
    for view in views {
        if view.kind != CommandKind::Shell {
            continue;
        }
        // replay_span_views は完了 span のみ返す。
        selected.push((view.command, view.exit_code));
    }

    if selected.len() > MAX_EVIDENCE_COMMANDS {
        let skip = selected.len() - MAX_EVIDENCE_COMMANDS;
        selected = selected.split_off(skip);
        dropped_old = true;
    }

    let mut truncated = source_truncated || dropped_old;
    let mut commands: Vec<HumanTaskCommandEvidence> = Vec::new();
    let mut total_bytes = 0usize;

    // 直近優先のため後ろから取り込み、最後に時系列へ戻す。
    let mut kept_rev: Vec<(String, Option<i32>, bool)> = Vec::new();
    for (command, exit_code) in selected.into_iter().rev() {
        let (command, cmd_truncated) = truncate_command(&command);
        let cmd_bytes = command.len();
        if total_bytes + cmd_bytes > MAX_EVIDENCE_TOTAL_COMMAND_BYTES {
            truncated = true;
            break;
        }
        total_bytes += cmd_bytes;
        if cmd_truncated {
            truncated = true;
        }
        kept_rev.push((command, exit_code, cmd_truncated));
    }
    kept_rev.reverse();

    for (index, (command, exit_code, _)) in kept_rev.into_iter().enumerate() {
        commands.push(HumanTaskCommandEvidence {
            index: index as u32,
            command,
            exit_code,
        });
    }

    Ok(HumanTaskEvidence {
        commands,
        truncated,
    })
}

fn truncate_command(command: &str) -> (String, bool) {
    if command.len() <= MAX_EVIDENCE_COMMAND_BYTES {
        return (command.to_string(), false);
    }
    let ellipsis_len = ELLIPSIS.len();
    let budget = MAX_EVIDENCE_COMMAND_BYTES.saturating_sub(ellipsis_len);
    let mut end = budget.min(command.len());
    while end > 0 && !command.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = command[..end].to_string();
    out.push_str(ELLIPSIS);
    debug_assert!(out.len() <= MAX_EVIDENCE_COMMAND_BYTES);
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aish_replay::{CommandKind, CommandSpec, LogEvent};

    fn shell_pair(index: u32, command: &str, exit_code: i32) -> [LogEvent; 2] {
        [
            LogEvent::shell_command_start(index, &format!("t{index}"), command),
            LogEvent::command_end(index, Some(exit_code), &format!("f{index}")),
        ]
    }

    fn exec_pair(index: u32, program: &str, exit_code: i32) -> [LogEvent; 2] {
        let spec = CommandSpec {
            program: program.into(),
            args: vec![],
        };
        [
            LogEvent::command_start_span(&spec, index, &format!("t{index}"), CommandKind::Exec),
            LogEvent::command_end(index, Some(exit_code), &format!("f{index}")),
        ]
    }

    #[test]
    fn build_maps_complete_shell_spans() {
        let mut events = Vec::new();
        events.extend(shell_pair(1, "printf ok", 0));
        events.extend(shell_pair(2, "false", 1));
        let evidence = build_human_task_evidence(&events, false).unwrap();
        assert_eq!(evidence.commands.len(), 2);
        assert_eq!(evidence.commands[0].command, "printf ok");
        assert_eq!(evidence.commands[0].exit_code, Some(0));
        assert_eq!(evidence.commands[0].index, 0);
        assert_eq!(evidence.commands[1].command, "false");
        assert_eq!(evidence.commands[1].exit_code, Some(1));
        assert!(!evidence.truncated);
    }

    #[test]
    fn build_excludes_non_shell_and_incomplete() {
        let mut events = Vec::new();
        events.extend(exec_pair(1, "tool", 0));
        events.push(LogEvent::shell_command_start(2, "t2", "incomplete"));
        events.extend(shell_pair(3, "kept", 0));
        let evidence = build_human_task_evidence(&events, false).unwrap();
        assert_eq!(evidence.commands.len(), 1);
        assert_eq!(evidence.commands[0].command, "kept");
    }

    #[test]
    fn build_keeps_recent_when_over_max_commands() {
        let mut events = Vec::new();
        for i in 0..(MAX_EVIDENCE_COMMANDS as u32 + 3) {
            events.extend(shell_pair(i, &format!("cmd-{i}"), 0));
        }
        let evidence = build_human_task_evidence(&events, false).unwrap();
        assert_eq!(evidence.commands.len(), MAX_EVIDENCE_COMMANDS);
        assert!(evidence.truncated);
        assert_eq!(evidence.commands[0].command, "cmd-3");
        assert_eq!(
            evidence.commands.last().unwrap().command,
            format!("cmd-{}", MAX_EVIDENCE_COMMANDS as u32 + 2)
        );
        for (i, cmd) in evidence.commands.iter().enumerate() {
            assert_eq!(cmd.index, i as u32);
        }
    }

    #[test]
    fn build_truncates_long_command_on_utf8_boundary() {
        let long = "あ".repeat(2000); // 3 bytes each
        let mut events = Vec::new();
        events.extend(shell_pair(0, &long, 0));
        let evidence = build_human_task_evidence(&events, false).unwrap();
        assert!(evidence.truncated);
        let cmd = &evidence.commands[0].command;
        assert!(cmd.len() <= MAX_EVIDENCE_COMMAND_BYTES);
        assert!(cmd.ends_with(ELLIPSIS));
        assert!(cmd.is_char_boundary(cmd.len() - ELLIPSIS.len()));
    }

    #[test]
    fn build_respects_total_byte_budget_keeping_recent() {
        let chunk = "a".repeat(MAX_EVIDENCE_COMMAND_BYTES);
        let mut events = Vec::new();
        // 16 KiB / 2 KiB = 8 commands max if each is full size
        for i in 0..12u32 {
            events.extend(shell_pair(
                i,
                &format!("{i}-{}", &chunk[..chunk.len() - 4]),
                0,
            ));
        }
        let evidence = build_human_task_evidence(&events, false).unwrap();
        assert!(evidence.truncated);
        assert!(!evidence.commands.is_empty());
        let total: usize = evidence.commands.iter().map(|c| c.command.len()).sum();
        assert!(total <= MAX_EVIDENCE_TOTAL_COMMAND_BYTES);
        assert!(evidence.commands.last().unwrap().command.starts_with("11-"));
    }

    #[test]
    fn build_stops_at_first_total_budget_miss_keeping_recent_only() {
        // 1 command 上限で truncate されるため、合計予算を満たすフルサイズ列で検証する。
        let full = "x".repeat(MAX_EVIDENCE_COMMAND_BYTES);
        let n_full = MAX_EVIDENCE_TOTAL_COMMAND_BYTES / MAX_EVIDENCE_COMMAND_BYTES;
        let old_short = "old-short";
        let mut events = Vec::new();
        events.extend(shell_pair(0, old_short, 0));
        events.extend(shell_pair(1, &format!("m{full}"), 0)); // middle: 予算超過で弾かれる
        for i in 0..n_full as u32 {
            events.extend(shell_pair(2 + i, &format!("r{i}-{full}"), 0));
        }
        let evidence = build_human_task_evidence(&events, false).unwrap();
        assert!(evidence.truncated);
        assert_eq!(evidence.commands.len(), n_full);
        assert!(evidence.commands.iter().all(|c| c.command.starts_with('r')));
        assert!(
            !evidence.commands.iter().any(|c| c.command == old_short),
            "must not keep older short command after skipping a newer one: {evidence:?}"
        );
        assert!(
            !evidence.commands.iter().any(|c| c.command.starts_with('m')),
            "middle full command must be dropped when total budget is exhausted: {evidence:?}"
        );
    }

    #[test]
    fn build_no_spans_is_empty_success() {
        let evidence = build_human_task_evidence(&[], true).unwrap();
        assert!(evidence.commands.is_empty());
        assert!(evidence.truncated);
    }

    #[test]
    fn build_propagates_source_truncation() {
        let mut events = Vec::new();
        events.extend(shell_pair(0, "only", 0));
        let evidence = build_human_task_evidence(&events, true).unwrap();
        assert!(evidence.truncated);
        assert_eq!(evidence.commands.len(), 1);
    }

    #[test]
    fn build_preserves_sanitized_command_text() {
        let mut events = Vec::new();
        events.extend(shell_pair(
            0,
            "printf '%s\\n' 'APP_SECRET=collab-evidence-test-secret'",
            0,
        ));
        let evidence = build_human_task_evidence(&events, false).unwrap();
        let command = &evidence.commands[0].command;
        assert!(
            !command.contains("collab-evidence-test-secret"),
            "raw secret must not appear: {command}"
        );
        assert!(
            command.contains("APP_SECRET=[REDACTED]"),
            "expected sanitized secret form, got {command}"
        );
    }
}
