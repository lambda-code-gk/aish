//! Minimal synchronous human shell（0055）。

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapters::outbound::toml_config::AishConfig;
use crate::adapters::outbound::{
    create_shell_session, prune_old_sessions, resolve_sessions_parent, JsonlFileLog, PtyShell,
};
use crate::application::RunShell;
use crate::domain::{CommandSpec, LogEvent};
use crate::ports::outbound::SessionLog;

pub const HANDOFF_ENV_KEYS: [&str; 3] = [
    "AISH_CONTROL_MODE",
    "AISH_HANDOFF_PARENT_REQUEST",
    "AISH_HANDOFF_SUGGESTED_COMMAND",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanShellResult {
    pub normal_return: bool,
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

pub fn human_shell_result_from_marker(
    marker: crate::adapters::outbound::HumanReturnMarker,
    child_exit_code: i32,
) -> HumanShellResult {
    HumanShellResult {
        normal_return: true,
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

pub fn print_handoff_briefing() {
    let parent_request = std::env::var("AISH_HANDOFF_PARENT_REQUEST").unwrap_or_default();
    let suggested = std::env::var("AISH_HANDOFF_SUGGESTED_COMMAND").unwrap_or_default();
    let mut out = std::io::stderr().lock();
    let _ = writeln!(out, "Human control requested by the parent agent.");
    let _ = writeln!(out);
    let _ = writeln!(out, "Parent request:");
    let _ = writeln!(out, "  {parent_request}");
    let _ = writeln!(out);
    let _ = writeln!(out, "Suggested command:");
    let _ = writeln!(out, "  {suggested}");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Edit, run, replace, or ignore the command as needed.\nPress Ctrl+D or run `exit` to return control."
    );
}

pub fn run_human_shell(result_file: &Path) -> anyhow::Result<HumanShellResult> {
    validate_handoff_environment()?;
    print_handoff_briefing();
    let cfg = AishConfig::load();
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
    let mut result = human_shell_result_from_marker(marker, code);
    result.shell_session_id = layout.id;
    result.shell_session_dir = layout.dir;
    result.shell_log_start = shell_log_start;
    result.shell_log_end = std::fs::metadata(&layout.log_path)
        .map(|m| m.len())
        .unwrap_or(shell_log_start);
    write_result(result_file, &result)?;
    Ok(result)
}

fn write_result(path: &Path, result: &HumanShellResult) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(path, format!("{json}\n"))?;
    Ok(())
}
