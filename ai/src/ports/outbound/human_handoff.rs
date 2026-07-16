//! Minimal human handoff ports（0055 / 0057）。

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use aibe_protocol::{HumanTaskBriefing, PostHandoffObservation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellOutcome {
    Done,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanShellLaunchRequest {
    pub cwd: PathBuf,
    pub parent_request_summary: String,
    pub suggested_command: String,
    pub runtime_dir: PathBuf,
    pub task_briefing: Option<HumanTaskBriefing>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HumanShellReturn {
    pub outcome: HumanShellOutcome,
    #[serde(default)]
    pub suspend_reason: Option<String>,
    pub exit_code: Option<i32>,
    pub final_cwd: PathBuf,
    #[serde(default)]
    pub shell_session_id: String,
    #[serde(default)]
    pub shell_session_dir: PathBuf,
    #[serde(default)]
    pub shell_log_start: u64,
    #[serde(default)]
    pub shell_log_end: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum HumanShellLaunchError {
    #[error("human shell cwd does not exist: {0}")]
    MissingCwd(String),
    /// Human Shell がまだ開始していない段階の失敗（cwd/runtime/spawn 等）。
    #[error("failed before launching human shell: {0}")]
    PreLaunchFailed(String),
    #[error("failed to launch human shell: {0}")]
    Failed(String),
    #[error("human shell ended without a normal return marker")]
    MissingReturnMarker,
    #[error("human handoff was interrupted: {0}")]
    Interrupted(String),
    #[error("human handoff was cancelled: {0}")]
    Cancelled(String),
    #[error("human task suspended")]
    Suspended {
        returned: Box<HumanShellReturn>,
        reason: Option<String>,
    },
}

pub trait HumanShellLauncher: Send + Sync {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
        cancel_requested: &AtomicBool,
    ) -> Result<HumanShellReturn, HumanShellLaunchError>;
}

pub trait EnvironmentObserver: Send + Sync {
    fn observe(
        &self,
        cwd: &Path,
        shell_log_start: u64,
        shell_log_end: Option<u64>,
        shell_session_dir: Option<&Path>,
    ) -> PostHandoffObservation;
}

pub trait ShellTranscriptReader: Send + Sync {
    fn read_tail(
        &self,
        session_dir: &Path,
        start: u64,
        end: Option<u64>,
    ) -> Result<(String, bool), String>;
}
