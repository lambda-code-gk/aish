//! Collaborative handoff の副作用境界（0055 Phase 2）。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanShellLaunchRequest {
    pub handoff_id: String,
    pub token: String,
    pub context_version: u32,
    pub cwd: PathBuf,
    pub suggestion_cache_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HumanShellReturn {
    pub normal_return: bool,
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
    #[error("failed to launch human shell: {0}")]
    Failed(String),
    #[error("human shell ended without a normal return marker")]
    MissingReturnMarker,
}

pub trait HumanShellLauncher: Send + Sync {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
    ) -> Result<HumanShellReturn, HumanShellLaunchError>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnvironmentObservation {
    pub cwd_exists: bool,
    pub cwd: String,
    pub git_head: Option<String>,
    pub git_branch: Option<String>,
    pub git_status: Option<String>,
    pub shell_log_end: Option<u64>,
}

pub trait EnvironmentObserver: Send + Sync {
    fn observe(&self, cwd: &Path, shell_log_start: u64) -> EnvironmentObservation;
}

/// 同一 parent turn で先に開始済みの tool が完了したことを保証する境界。
pub trait ParentToolBarrier: Send + Sync {
    fn wait_for_started_tools(&self) -> Result<(), String>;
}

pub trait HandoffCandidatePublisher: Send + Sync {
    fn publish(&self, handoff_id: &str, commands: &[String]) -> Result<(), String>;
}

pub trait HandoffRuntime: Send + Sync {
    fn now_ms(&self) -> u64;
    fn unique_id(&self, prefix: &str) -> String;
    fn secure_token(&self) -> Result<String, String>;
    fn host_id(&self) -> String {
        "unknown-host".into()
    }
    fn effective_uid(&self) -> u32 {
        0
    }
    fn process_id(&self) -> u32 {
        0
    }
    fn tty(&self) -> Option<String> {
        None
    }
    fn process_is_alive(&self, process_id: u32) -> bool {
        process_id != 0
    }
    fn handoff_suggestion_cache_path(&self, handoff_id: &str) -> PathBuf {
        PathBuf::from("/tmp")
            .join("ai-suggestions")
            .join(format!("handoff-{handoff_id}.json"))
    }
}

#[derive(Debug, Default)]
pub struct NoopHandoffCandidatePublisher;

impl HandoffCandidatePublisher for NoopHandoffCandidatePublisher {
    fn publish(&self, _handoff_id: &str, _commands: &[String]) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopParentToolBarrier;

impl ParentToolBarrier for NoopParentToolBarrier {
    fn wait_for_started_tools(&self) -> Result<(), String> {
        Ok(())
    }
}
