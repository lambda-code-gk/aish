//! Minimal synchronous human shell（0055）。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapters::outbound::toml_config::AishConfig;
use crate::adapters::outbound::{
    create_shell_session, detect_child_shell, prune_old_sessions, resolve_sessions_parent,
    ChildShellKind, JsonlFileLog, PtyShell,
};
use crate::application::RunShell;
use crate::domain::{CommandSpec, LogEvent};
use crate::ports::outbound::SessionLog;

pub const HANDOFF_ENV_KEYS: [&str; 5] = [
    "AISH_CONTROL_MODE",
    "AISH_HANDOFF_PARENT_REQUEST",
    "AISH_HANDOFF_SUGGESTED_COMMAND",
    "AISH_HANDOFF_RUNTIME_DIR",
    "AISH_HANDOFF_TASK_JSON",
];

const HUMAN_TASK_BRIEFING_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskBriefing {
    version: u8,
    objective: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    instructions: Vec<String>,
    #[serde(default)]
    completion_criteria: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellOutcome {
    Done,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanShellResult {
    pub outcome: HumanShellOutcome,
    pub exit_code: Option<i32>,
    pub final_cwd: PathBuf,
    pub shell_session_id: String,
    pub shell_session_dir: PathBuf,
    pub shell_log_start: u64,
    pub shell_log_end: u64,
}

pub fn handoff_environment_is_complete<'a>(
    values: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> bool {
    values
        .into_iter()
        .any(|(key, value)| key == "AISH_CONTROL_MODE" && value == "human-shell")
}

/// 親リクエスト・候補コマンド表示用に制御文字を escape する。
/// UTF-8 テキスト（日本語等）はそのまま表示し、ANSI / C0 制御文字のみ無害化する。
pub fn escape_for_handoff_display(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                let code = ch as u32;
                if code <= 0x7f {
                    out.push_str(&format!("\\x{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{{{code:04x}}}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out
}

/// 複数行文字列を論理行ごとに escape し、先頭2空白でインデントする。
/// 文字列全体を先に escape して改行を `\\n` に潰さない。
pub fn format_indented_block(value: &str) -> String {
    value
        .split('\n')
        .map(|line| format!("  {}", escape_for_handoff_display(line)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Human Task briefing を引数だけから生成する純粋関数。
pub fn render_human_task_briefing(parent_request: &str, suggested_command: &str) -> String {
    let objective = if parent_request.trim().is_empty() {
        format_indented_block("No parent request summary is available.")
    } else {
        format_indented_block(parent_request)
    };
    let suggested = if suggested_command.trim().is_empty() {
        format_indented_block("No command was provided.")
    } else {
        format_indented_block(suggested_command)
    };

    let mut out = String::new();
    out.push_str("AISH Collaborative Mode\n");
    out.push_str("=======================\n");
    out.push('\n');
    out.push_str("Human Task\n");
    out.push('\n');
    out.push_str("Objective:\n");
    out.push_str(&objective);
    out.push('\n');
    out.push('\n');
    out.push_str("Why this is a Human Task:\n");
    out.push_str("  The parent agent requested a shell operation in Collab Mode.\n");
    out.push_str("  AISH has not automatically executed the requested command.\n");
    out.push('\n');
    out.push_str("Suggested first action:\n");
    out.push_str(&suggested);
    out.push('\n');
    out.push('\n');
    out.push_str("Done when:\n");
    out.push_str("  Return control after you have completed the necessary work,\n");
    out.push_str("  or when the parent agent should re-observe the environment\n");
    out.push_str("  and decide the next step.\n");
    out.push('\n');
    out.push_str("You remain in control:\n");
    out.push_str("  Edit, run, replace, or ignore the suggested command.\n");
    out.push_str("  Alt+. or Alt+, inserts the suggested command.\n");
    out.push_str("  Press Ctrl+D or run `exit` to return control.\n");
    out
}

pub fn render_explicit_human_task(task: &HumanTaskBriefing) -> String {
    let mut out = String::from(
        "AISH Collaborative Mode\n=======================\n\nHuman Task\n\nObjective:\n",
    );
    out.push_str(&format_indented_block(&task.objective));
    out.push('\n');
    if let Some(reason) = task.reason.as_deref() {
        out.push_str("\nWhy this is a Human Task:\n");
        out.push_str(&format_indented_block(reason));
        out.push('\n');
    }
    if !task.instructions.is_empty() {
        out.push_str("\nSuggested actions:\n");
        for item in &task.instructions {
            out.push_str(&format_list_item(item));
            out.push('\n');
        }
    }
    if !task.completion_criteria.is_empty() {
        out.push_str("\nDone when:\n");
        for item in &task.completion_criteria {
            out.push_str(&format_list_item(item));
            out.push('\n');
        }
    }
    out.push_str("\nYou remain in control:\n  Press Ctrl+D or run `exit` to return control.\n");
    out
}

fn format_list_item(value: &str) -> String {
    value
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            let prefix = if index == 0 { "  - " } else { "    " };
            format!("{prefix}{}", escape_for_handoff_display(line))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_explicit_briefing(raw: &str) -> anyhow::Result<HumanTaskBriefing> {
    if raw.len() > HUMAN_TASK_BRIEFING_MAX_BYTES {
        anyhow::bail!("human task briefing exceeds 64 KiB");
    }
    let task: HumanTaskBriefing =
        serde_json::from_str(raw).map_err(|_| anyhow::anyhow!("invalid human task briefing"))?;
    if task.version != 1 {
        anyhow::bail!("unsupported human task briefing version");
    }
    if task.objective.trim().is_empty() {
        anyhow::bail!("human task objective is empty");
    }
    Ok(task)
}

pub fn human_shell_result_from_marker(
    marker: crate::adapters::outbound::HumanReturnMarker,
    child_exit_code: i32,
) -> HumanShellResult {
    HumanShellResult {
        outcome: if marker.suspended {
            HumanShellOutcome::Suspended
        } else {
            HumanShellOutcome::Done
        },
        exit_code: marker.exit_code.or(Some(child_exit_code)),
        final_cwd: PathBuf::from(marker.final_cwd),
        shell_session_id: String::new(),
        shell_session_dir: PathBuf::new(),
        shell_log_start: 0,
        shell_log_end: 0,
    }
}

pub fn validate_handoff_environment() -> anyhow::Result<()> {
    if std::env::var("AISH_CONTROL_MODE").as_deref() != Ok("human-shell") {
        anyhow::bail!("AISH_CONTROL_MODE must be human-shell");
    }
    Ok(())
}

pub fn validate_handoff_shell(shell: &str) -> anyhow::Result<()> {
    match detect_child_shell(shell) {
        ChildShellKind::Bash | ChildShellKind::Zsh => Ok(()),
        ChildShellKind::Other => {
            anyhow::bail!("minimal human handoff currently supports bash and zsh only")
        }
    }
}

pub fn print_handoff_briefing() -> anyhow::Result<()> {
    if let Ok(raw) = std::env::var("AISH_HANDOFF_TASK_JSON") {
        let task = decode_explicit_briefing(&raw)?;
        let rendered = render_explicit_human_task(&task);
        write!(std::io::stderr(), "{rendered}")?;
        return Ok(());
    }
    let parent_request = std::env::var("AISH_HANDOFF_PARENT_REQUEST").unwrap_or_default();
    let suggested = std::env::var("AISH_HANDOFF_SUGGESTED_COMMAND").unwrap_or_default();
    let rendered = render_human_task_briefing(&parent_request, &suggested);
    let _ = write!(std::io::stderr(), "{rendered}");
    Ok(())
}

pub fn run_human_shell(result_file: &Path) -> anyhow::Result<HumanShellResult> {
    validate_handoff_environment()?;
    print_handoff_briefing()?;
    let cfg = AishConfig::load();
    validate_handoff_shell(&cfg.shell)?;
    let parent = resolve_sessions_parent(&cfg);
    let layout = create_shell_session(&parent)?;
    prune_old_sessions(&parent, cfg.max_sessions)?;
    let shell = cfg.shell;
    let mut log = JsonlFileLog::new(layout.log_path.clone());
    let shell_log_start = std::fs::metadata(&layout.log_path)
        .map(|m| m.len())
        .unwrap_or(0);
    log.append(&LogEvent::command_start(&CommandSpec {
        program: "human_shell".into(),
        args: vec![shell.clone()],
    }))?;
    let mut runner = PtyShell::new(&mut log);
    let code = RunShell::new(&mut runner).run(&shell, &layout.dir)?;
    let marker = runner
        .take_human_return_marker()
        .ok_or_else(|| anyhow::anyhow!("human shell ended without normal return marker"))?;
    log.append(&LogEvent::Exit { code: Some(code) })?;
    let suspend_reason = marker.suspend_reason.clone();
    let suspended = marker.suspended;
    let mut result = human_shell_result_from_marker(marker, code);
    result.shell_session_id = layout.id;
    result.shell_session_dir = layout.dir;
    result.shell_log_start = shell_log_start;
    result.shell_log_end = std::fs::metadata(&layout.log_path)
        .map(|m| m.len())
        .unwrap_or(shell_log_start);
    write_result(result_file, &result)?;
    if suspended {
        write_suspend_reason(result_file, suspend_reason.as_deref())?;
    }
    Ok(result)
}

pub fn emit_human_suspend_control(cwd: &Path, reason: &str) -> anyhow::Result<()> {
    if reason.len() > 4096 || reason.chars().any(char::is_control) {
        anyhow::bail!("invalid suspend reason");
    }
    let cwd = cwd
        .to_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid suspend cwd"))?;
    let fifo = std::env::var_os("AISH_CONTROL_FIFO")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("human suspend control unavailable"))?;
    let metadata = std::fs::symlink_metadata(&fifo)
        .map_err(|_| anyhow::anyhow!("human suspend control unavailable"))?;
    if !std::os::unix::fs::FileTypeExt::is_fifo(&metadata.file_type()) {
        anyhow::bail!("human suspend control unavailable");
    }
    let mut file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(fifo)
        .map_err(|_| anyhow::anyhow!("human suspend control unavailable"))?;
    let event = serde_json::json!({
        "version": 1,
        "event": "human_suspend",
        "exit_code": 0,
        "cwd": cwd,
        "reason": reason,
    });
    serde_json::to_writer(&mut file, &event)
        .map_err(|_| anyhow::anyhow!("human suspend control unavailable"))?;
    file.write_all(b"\n")
        .map_err(|_| anyhow::anyhow!("human suspend control unavailable"))?;
    Ok(())
}

fn write_suspend_reason(result_file: &Path, reason: Option<&str>) -> anyhow::Result<()> {
    let Some(parent) = result_file.parent() else {
        anyhow::bail!("missing result parent");
    };
    let path = parent.join("suspend-reason");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(reason.unwrap_or("").as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn ensure_dir_0700(path: &Path) -> std::io::Result<()> {
    use std::fs::DirBuilder;
    use std::os::unix::fs::DirBuilderExt;

    let mut components = Vec::new();
    let mut cursor = path;
    while let Some(parent) = cursor.parent() {
        if cursor.as_os_str().is_empty() {
            break;
        }
        components.push(cursor);
        cursor = parent;
    }
    components.reverse();

    for component in components {
        if component.exists() {
            reject_symlink_dir(component)?;
            continue;
        }
        DirBuilder::new().mode(0o700).create(component)?;
    }
    Ok(())
}

fn reject_symlink_dir(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "refusing result path through symlink directory",
        ));
    }
    if !meta.is_dir() {
        return Err(std::io::Error::other(
            "result path component is not a directory",
        ));
    }
    Ok(())
}

fn ensure_private_dir_owned_0700(path: &Path) -> std::io::Result<()> {
    reject_symlink_dir(path)?;
    let meta = fs::metadata(path)?;
    let owner = meta.uid();
    let current = unsafe { libc::getuid() };
    if owner != current {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "private result directory must be owned by current user: {}",
                path.display()
            ),
        ));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn write_result(path: &Path, result: &HumanShellResult) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_0700(parent)?;
        ensure_private_dir_owned_0700(parent)?;
    }
    let json = serde_json::to_string_pretty(result)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| anyhow::anyhow!("refusing to follow result path symlink: {e}"))?;
    let meta = file.metadata()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(anyhow::anyhow!(
            "result file must be a regular file: {}",
            path.display()
        ));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(anyhow::anyhow!(
            "result file must be 0600, got {mode:o}: {}",
            path.display()
        ));
    }
    file.write_all(format!("{json}\n").as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_result_rejects_symlink_target() {
        use std::os::unix::fs::PermissionsExt;

        let base = tempfile::tempdir().unwrap();
        let parent = base.path().join("runtime");
        fs::create_dir_all(&parent).unwrap();
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&parent, perms).unwrap();
        let victim = base.path().join("victim.txt");
        fs::write(&victim, b"keep").unwrap();
        let result_path = parent.join("result.json");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, &result_path).unwrap();
        let result = HumanShellResult {
            outcome: HumanShellOutcome::Done,
            exit_code: Some(0),
            final_cwd: parent.clone(),
            shell_session_id: "test".into(),
            shell_session_dir: parent.clone(),
            shell_log_start: 0,
            shell_log_end: 0,
        };
        let err = write_result(&result_path, &result).expect_err("symlink result path");
        assert!(
            err.to_string().contains("symlink") || err.to_string().contains("follow"),
            "unexpected error: {err}"
        );
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep");
    }

    #[test]
    fn write_result_tightens_owned_insecure_parent() {
        use std::os::unix::fs::PermissionsExt;

        let base = tempfile::tempdir().unwrap();
        let parent = base.path().join("insecure");
        fs::create_dir_all(&parent).unwrap();
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o775);
        fs::set_permissions(&parent, perms).unwrap();
        let result_path = parent.join("result.json");
        let result = HumanShellResult {
            outcome: HumanShellOutcome::Done,
            exit_code: Some(0),
            final_cwd: parent.clone(),
            shell_session_id: "test".into(),
            shell_session_dir: parent.clone(),
            shell_log_start: 0,
            shell_log_end: 0,
        };
        write_result(&result_path, &result).expect("tighten owned parent");
        let parent_mode = fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
        assert_eq!(parent_mode, 0o700);
        assert!(result_path.is_file());
    }

    #[test]
    fn ensure_dir_0700_does_not_chmod_existing_parent() {
        let base = tempfile::tempdir().unwrap();
        let parent = base.path().join("shared");
        fs::create_dir_all(&parent).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&parent).unwrap().permissions();
            perms.set_mode(0o1777);
            fs::set_permissions(&parent, perms).unwrap();
            let before = fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
            let leaf = parent.join("nested").join("result-parent");
            ensure_dir_0700(&leaf).expect("create nested private dir");
            let after = fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
            assert_eq!(before, after, "existing parent must not be chmodded");
            let nested_mode = fs::metadata(parent.join("nested"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(nested_mode, 0o700);
        }
    }

    #[test]
    fn briefing_preserves_utf8_japanese() {
        let raw = "shell_exec でファイルのリストを取得";
        let escaped = escape_for_handoff_display(raw);
        assert_eq!(escaped, raw);
        let displayed = format_indented_block(raw);
        assert!(displayed.contains("ファイル"));
        assert!(!displayed.contains("\\xe3"));
    }

    #[test]
    fn briefing_escapes_ansi_control_sequences() {
        let raw = "line1\x1b[31mred\x07bell\ttab";
        let escaped = escape_for_handoff_display(raw);
        assert!(!escaped.contains('\x1b'));
        assert!(!escaped.contains('\x07'));
        assert!(!escaped.contains('\t'));
        assert!(escaped.contains("\\x1b"));
        let displayed = format_indented_block(raw);
        assert!(!displayed.contains('\x1b'));
        assert!(displayed.contains("\\x1b"));
    }

    #[test]
    fn briefing_cannot_emit_terminal_title_or_osc_sequence() {
        let osc = "\x1b]0;evil title\x07";
        let escaped = escape_for_handoff_display(osc);
        assert!(!escaped.contains('\x1b'));
        assert!(!escaped.contains('\x07'));
        let displayed = format_indented_block(osc);
        assert!(!displayed.contains('\x1b'));
        assert!(!displayed.contains('\x07'));
    }

    #[test]
    fn format_indented_block_preserves_logical_newlines() {
        let displayed = format_indented_block("line one\nline two");
        assert_eq!(displayed, "  line one\n  line two");
        assert!(!displayed.contains("\\n"));
    }

    #[test]
    fn explicit_human_task_omits_empty_sections() {
        let task = decode_explicit_briefing(r#"{"version":1,"objective":"inspect"}"#).unwrap();
        let rendered = render_explicit_human_task(&task);
        assert!(rendered.contains("Objective:\n  inspect"));
        assert!(!rendered.contains("Why this is a Human Task:"));
        assert!(!rendered.contains("Suggested actions:"));
        assert!(!rendered.contains("Done when:"));
    }

    #[test]
    fn explicit_human_task_rejects_invalid_version_and_size() {
        assert!(decode_explicit_briefing(r#"{"version":2,"objective":"x"}"#).is_err());
        assert!(decode_explicit_briefing(&"x".repeat(HUMAN_TASK_BRIEFING_MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn explicit_human_task_indents_each_list_item_line() {
        let task = decode_explicit_briefing(
            r#"{"version":1,"objective":"inspect","instructions":["first\nsecond"],"completion_criteria":["done\nverified"]}"#,
        )
        .unwrap();
        let rendered = render_explicit_human_task(&task);
        assert!(rendered.contains("Suggested actions:\n  - first\n    second"));
        assert!(rendered.contains("Done when:\n  - done\n    verified"));
        assert!(!rendered.contains("first\\nsecond"));
    }
}
