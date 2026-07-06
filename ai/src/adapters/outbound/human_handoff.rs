//! Minimal human handoff adapters（0055）。

use std::path::{Path, PathBuf};
use std::process::Command;

use aibe_protocol::PostHandoffObservation;

use crate::domain::HANDOFF_ENV_KEYS;
use crate::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn, ShellTranscriptReader,
};

#[derive(Debug, Clone)]
pub struct AishHumanShellLauncher {
    binary: PathBuf,
}

impl Default for AishHumanShellLauncher {
    fn default() -> Self {
        let sibling = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("aish")))
            .filter(|path| path.is_file());
        Self {
            binary: std::env::var_os("AISH_BIN")
                .map(PathBuf::from)
                .or(sibling)
                .unwrap_or_else(|| PathBuf::from("aish")),
        }
    }
}

impl AishHumanShellLauncher {
    pub fn new(binary: PathBuf) -> Self {
        Self { binary }
    }
}

impl HumanShellLauncher for AishHumanShellLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        if !request.cwd.is_dir() {
            return Err(HumanShellLaunchError::MissingCwd(
                request.cwd.display().to_string(),
            ));
        }
        std::fs::create_dir_all(&request.runtime_dir)
            .map_err(|e| HumanShellLaunchError::Failed(format!("runtime dir: {e}")))?;
        let result_path = request.runtime_dir.join("result.json");
        let mut child = Command::new(&self.binary);
        child
            .arg("human-shell")
            .arg("--result-file")
            .arg(&result_path)
            .current_dir(&request.cwd)
            .env("AISH_CONTROL_MODE", "human-shell")
            .env(
                "AISH_HANDOFF_PARENT_REQUEST",
                &request.parent_request_summary,
            )
            .env("AISH_HANDOFF_SUGGESTED_COMMAND", &request.suggested_command)
            .env("AISH_HANDOFF_RUNTIME_DIR", &request.runtime_dir);
        strip_handoff_environment(&mut child);
        let status = child
            .status()
            .map_err(|e| HumanShellLaunchError::Failed(e.to_string()))?;
        if !status.success() && !result_path.is_file() {
            return Err(HumanShellLaunchError::Interrupted(format!(
                "Human handoff was interrupted.\nRestart the original request. (exit={:?})",
                status.code()
            )));
        }
        let raw = std::fs::read_to_string(&result_path).map_err(|_| {
            HumanShellLaunchError::Interrupted(
                "Human handoff was interrupted.\nRestart the original request.".into(),
            )
        })?;
        let mut returned: HumanShellReturn = serde_json::from_str(raw.trim())
            .map_err(|e| HumanShellLaunchError::Failed(e.to_string()))?;
        if !returned.normal_return {
            return Err(HumanShellLaunchError::MissingReturnMarker);
        }
        if returned.exit_code.is_none() {
            returned.exit_code = status.code();
        }
        Ok(returned)
    }
}

#[derive(Debug, Default)]
pub struct ProcessEnvironmentObserver {
    transcript: FileShellTranscriptReader,
}

impl EnvironmentObserver for ProcessEnvironmentObserver {
    fn observe(
        &self,
        cwd: &Path,
        shell_log_start: u64,
        shell_log_end: Option<u64>,
        shell_session_dir: Option<&Path>,
    ) -> PostHandoffObservation {
        let cwd_exists = cwd.is_dir();
        let mut observation_errors = Vec::new();
        let git = |args: &[&str]| -> Option<String> {
            if !cwd_exists {
                return None;
            }
            let output = Command::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .ok()?;
            if output.status.success() {
                Some(
                    String::from_utf8_lossy(&output.stdout)
                        .trim_end()
                        .to_string(),
                )
            } else {
                None
            }
        };
        let is_git_repo = git(&["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true");
        let shell_log_tail = shell_session_dir.and_then(|dir| {
            self.transcript
                .read_tail(dir, shell_log_start, shell_log_end)
                .map_err(|error| observation_errors.push(error))
                .ok()
        });
        PostHandoffObservation {
            cwd_exists,
            cwd: cwd.display().to_string(),
            git_head: is_git_repo.then(|| git(&["rev-parse", "HEAD"])).flatten(),
            git_branch: is_git_repo
                .then(|| git(&["branch", "--show-current"]))
                .flatten(),
            git_status: is_git_repo.then(|| git(&["status", "--short"])).flatten(),
            shell_log_tail,
            observation_errors,
        }
    }
}

#[derive(Debug, Default)]
pub struct FileShellTranscriptReader;

impl ShellTranscriptReader for FileShellTranscriptReader {
    fn read_tail(
        &self,
        session_dir: &Path,
        start: u64,
        end: Option<u64>,
    ) -> Result<String, String> {
        let path = session_dir.join("log.jsonl");
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let end = end.unwrap_or(bytes.len() as u64) as usize;
        let start = start.min(end as u64) as usize;
        if start >= bytes.len() {
            return Ok(String::new());
        }
        let slice = &bytes[start.min(bytes.len())..end.min(bytes.len())];
        Ok(String::from_utf8_lossy(slice).into_owned())
    }
}

fn strip_handoff_environment(command: &mut Command) {
    for key in HANDOFF_ENV_KEYS {
        command.env_remove(key);
    }
}

pub fn create_runtime_handoff_dir() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("TMPDIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join("aish").join(format!(
        "handoff-{}-{}",
        std::process::id(),
        rand_handoff_suffix()
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn cleanup_runtime_handoff_dir(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
}

fn rand_handoff_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:x}")
}
