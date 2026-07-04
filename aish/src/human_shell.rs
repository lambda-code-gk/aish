//! Collaborative handoff 用 PTY human shell（0055 Phase 2）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapters::outbound::toml_config::AishConfig;
use crate::adapters::outbound::{
    create_shell_session, prune_old_sessions, resolve_sessions_parent, JsonlFileLog, PtyShell,
};
use crate::application::RunShell;
use crate::domain::{CommandSpec, LogEvent};
use crate::ports::outbound::SessionLog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanShellResult {
    pub normal_return: bool,
    pub exit_code: Option<i32>,
    pub final_cwd: PathBuf,
}

pub const HANDOFF_ENV_KEYS: [&str; 4] = [
    "AISH_CONTROL_MODE",
    "AISH_HANDOFF_ID",
    "AISH_HANDOFF_TOKEN",
    "AISH_HANDOFF_CONTEXT_VERSION",
];

pub fn handoff_environment_is_complete<'a>(
    values: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> bool {
    let values: std::collections::HashMap<&str, &str> = values.into_iter().collect();
    values.get("AISH_CONTROL_MODE") == Some(&"human-shell")
        && HANDOFF_ENV_KEYS
            .iter()
            .all(|key| values.get(key).is_some_and(|value| !value.is_empty()))
}

pub fn human_shell_result_from_marker(
    marker: crate::adapters::outbound::HumanReturnMarker,
    child_exit_code: i32,
) -> HumanShellResult {
    HumanShellResult {
        normal_return: true,
        exit_code: marker.exit_code.or(Some(child_exit_code)),
        final_cwd: PathBuf::from(marker.final_cwd),
    }
}

pub fn validate_handoff_environment() -> anyhow::Result<()> {
    for key in HANDOFF_ENV_KEYS.iter().skip(1) {
        if std::env::var_os(key).is_none() {
            anyhow::bail!("missing required human-shell environment variable {key}");
        }
    }
    if std::env::var("AISH_CONTROL_MODE").as_deref() != Ok("human-shell") {
        anyhow::bail!("AISH_CONTROL_MODE must be human-shell");
    }
    Ok(())
}

pub fn run_human_shell(result_file: &Path) -> anyhow::Result<HumanShellResult> {
    validate_handoff_environment()?;
    let cfg = AishConfig::load();
    let parent = resolve_sessions_parent(&cfg);
    let layout = create_shell_session(&parent)?;
    prune_old_sessions(&parent, cfg.max_sessions)?;
    let shell = cfg.shell;
    let mut log = JsonlFileLog::new(layout.log_path.clone());
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
    let result = human_shell_result_from_marker(marker, code);
    write_result(result_file, &result)?;
    Ok(result)
}

fn write_result(path: &Path, result: &HumanShellResult) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    serde_json::to_writer(&mut file, result)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
