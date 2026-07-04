//! Collaborative handoff の process / environment adapters。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::toml_config::AiConfig;
use super::FileSuggestedCommandRecallStore;
use crate::domain::{SuggestedCommandCache, SuggestedCommandCandidate, SuggestedCommandQueue};
use crate::ports::outbound::{
    EnvironmentObservation, EnvironmentObserver, HandoffCandidatePublisher, HandoffRuntime,
    HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher, HumanShellReturn,
    SuggestedCommandRecallStore,
};

pub fn strip_handoff_environment(command: &mut Command) {
    for key in crate::application::HANDOFF_ENV_KEYS {
        command.env_remove(key);
    }
}

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
        let result_file = tempfile::Builder::new()
            .prefix("aish-handoff-result-")
            .tempfile()
            .map_err(|e| HumanShellLaunchError::Failed(e.to_string()))?;
        let result_path = result_file.path().to_path_buf();
        drop(result_file);
        let collaborative = AiConfig::load().collaborative;
        let status = Command::new(&self.binary)
            .arg("human-shell")
            .arg("--result-file")
            .arg(&result_path)
            .current_dir(&request.cwd)
            .env("AISH_CONTROL_MODE", "human-shell")
            .env("AISH_HANDOFF_ID", &request.handoff_id)
            .env("AISH_HANDOFF_TOKEN", &request.token)
            .env("AI_SUGGESTION_CACHE", &request.suggestion_cache_path)
            .env(
                "AISH_HANDOFF_CONTEXT_VERSION",
                request.context_version.to_string(),
            )
            .env(
                "AISH_HANDOFF_STORE_ROOT",
                crate::adapters::outbound::FilesystemHandoffStore::default_root(),
            )
            .env(
                "AISH_HANDOFF_HEARTBEAT_INTERVAL_MS",
                (collaborative.heartbeat_interval_secs * 1000).to_string(),
            )
            .env(
                "AISH_HANDOFF_LEASE_TIMEOUT_MS",
                (collaborative.lease_timeout_secs * 1000).to_string(),
            )
            .env(
                "AISH_COLLABORATIVE_PROMPT_TEMPLATE",
                collaborative.prompt_template,
            )
            .status()
            .map_err(|e| HumanShellLaunchError::Failed(e.to_string()))?;
        let raw = std::fs::read_to_string(&result_path)
            .map_err(|_| HumanShellLaunchError::MissingReturnMarker)?;
        let _ = std::fs::remove_file(&result_path);
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
pub struct ProcessEnvironmentObserver;

impl EnvironmentObserver for ProcessEnvironmentObserver {
    fn observe(&self, cwd: &Path, _shell_log_start: u64) -> EnvironmentObservation {
        let cwd_exists = cwd.is_dir();
        let git = |args: &[&str]| -> Option<String> {
            if !cwd_exists {
                return None;
            }
            let output = Command::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .ok()?;
            output.status.success().then(|| {
                String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_string()
            })
        };
        EnvironmentObservation {
            cwd_exists,
            cwd: cwd.display().to_string(),
            git_head: git(&["rev-parse", "HEAD"]),
            git_branch: git(&["branch", "--show-current"]),
            git_status: git(&["status", "--short"]),
            shell_log_end: std::env::var_os("AISH_SESSION_DIR")
                .map(PathBuf::from)
                .and_then(|dir| std::fs::metadata(dir.join("log.jsonl")).ok())
                .map(|metadata| metadata.len()),
        }
    }
}

pub struct FileHandoffCandidatePublisher;

impl FileHandoffCandidatePublisher {
    pub fn new(_store: FileSuggestedCommandRecallStore, _ai_session_id: String) -> Self {
        Self
    }
}

impl HandoffCandidatePublisher for FileHandoffCandidatePublisher {
    fn publish(&self, handoff_id: &str, commands: &[String]) -> Result<(), String> {
        if commands.is_empty() {
            return Ok(());
        }
        let runtime = SystemHandoffRuntime;
        let store =
            FileSuggestedCommandRecallStore::new(runtime.handoff_suggestion_cache_path(handoff_id));
        let captured_at = runtime.now_ms().to_string();
        let mut cache = store.load().map_err(|e| e.to_string())?.unwrap_or_else(|| {
            SuggestedCommandCache::new(handoff_id, interactive_shell_name(), &captured_at)
        });
        cache.updated_at = captured_at.clone();
        cache.append_queue(SuggestedCommandQueue {
            turn_id: format!("handoff:{handoff_id}"),
            captured_at,
            candidates: commands
                .iter()
                .map(|command| SuggestedCommandCandidate {
                    text: command.clone(),
                    language: "shell".into(),
                    bytes: command.len(),
                })
                .collect(),
        });
        store.save(&cache).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Default)]
pub struct SystemHandoffRuntime;

impl HandoffRuntime for SystemHandoffRuntime {
    fn now_ms(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn unique_id(&self, prefix: &str) -> String {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{prefix}-{}-{}-{seq}", self.now_ms(), std::process::id())
    }

    fn secure_token(&self) -> Result<String, String> {
        use std::io::Read;
        let mut bytes = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|e| e.to_string())?;
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    fn host_id(&self) -> String {
        let mut buffer = [0_u8; 256];
        let rc = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
        if rc != 0 {
            return "unknown-host".into();
        }
        let len = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        String::from_utf8_lossy(&buffer[..len]).into_owned()
    }

    fn effective_uid(&self) -> u32 {
        unsafe { libc::geteuid() }
    }

    fn process_id(&self) -> u32 {
        std::process::id()
    }

    fn tty(&self) -> Option<String> {
        std::env::var("TTY").ok()
    }

    fn process_is_alive(&self, process_id: u32) -> bool {
        if process_id == 0 {
            return false;
        }
        let result = unsafe { libc::kill(process_id as libc::pid_t, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    fn handoff_suggestion_cache_path(&self, handoff_id: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join(".local/share/ai/suggestions")
            .join(format!("handoff-{handoff_id}.json"))
    }
}

fn interactive_shell_name() -> String {
    std::env::var("SHELL")
        .ok()
        .and_then(|p| {
            PathBuf::from(p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "sh".into())
}
