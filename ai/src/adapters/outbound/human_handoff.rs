//! Minimal human handoff adapters（0055）。

use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use aibe_protocol::PostHandoffObservation;

use crate::domain::HANDOFF_ENV_KEYS;
use crate::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn, ShellTranscriptReader,
};

/// shell log tail の最大読み込みサイズ。
pub const SHELL_LOG_TAIL_MAX_BYTES: usize = 32 * 1024;

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
        ensure_dir_0700(&request.runtime_dir)
            .map_err(|e| HumanShellLaunchError::Failed(format!("runtime dir: {e}")))?;
        ensure_private_dir_owned_0700(&request.runtime_dir)
            .map_err(|e| HumanShellLaunchError::Failed(format!("runtime dir permissions: {e}")))?;
        let result_path = request.runtime_dir.join("result.json");
        let mut child = Command::new(&self.binary);
        child
            .arg("human-shell")
            .arg("--result-file")
            .arg(&result_path)
            .current_dir(&request.cwd);
        // 親 ai からの handoff 環境変数を除去してから、子 aish 用の値を設定する。
        strip_handoff_environment(&mut child);
        child
            .env("AISH_CONTROL_MODE", "human-shell")
            .env(
                "AISH_HANDOFF_PARENT_REQUEST",
                &request.parent_request_summary,
            )
            .env("AISH_HANDOFF_SUGGESTED_COMMAND", &request.suggested_command)
            .env("AISH_HANDOFF_RUNTIME_DIR", &request.runtime_dir);
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
                .map(|(tail, truncated)| {
                    if truncated {
                        observation_errors.push("shell_log_tail_truncated".into());
                    }
                    tail
                })
                .map_err(|error| observation_errors.push(error))
                .ok()
        });
        let shell_log_truncated = if observation_errors
            .iter()
            .any(|e| e == "shell_log_tail_truncated")
        {
            Some(true)
        } else {
            None
        };
        PostHandoffObservation {
            cwd_exists,
            cwd: cwd.display().to_string(),
            git_head: is_git_repo.then(|| git(&["rev-parse", "HEAD"])).flatten(),
            git_branch: is_git_repo
                .then(|| git(&["branch", "--show-current"]))
                .flatten(),
            git_status: is_git_repo.then(|| git(&["status", "--short"])).flatten(),
            shell_log_tail,
            shell_log_truncated,
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
    ) -> Result<(String, bool), String> {
        let path = session_dir.join("log.jsonl");
        let file_len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        let end = end.unwrap_or(file_len).min(file_len);
        let start = start.min(end);
        let span = end.saturating_sub(start);
        let (read_start, truncated_by_max) = if span as usize > SHELL_LOG_TAIL_MAX_BYTES {
            (end - SHELL_LOG_TAIL_MAX_BYTES as u64, true)
        } else {
            (start, false)
        };
        let read_start = read_start.max(start);
        let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(read_start))
            .map_err(|e| e.to_string())?;
        let to_read = (end - read_start) as usize;
        let mut buf = vec![0u8; to_read];
        file.read_exact(&mut buf).map_err(|e| e.to_string())?;
        let truncated = truncated_by_max || read_start > start;
        Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
    }
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
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "refusing handoff path through symlink directory",
        ));
    }
    if !meta.is_dir() {
        return Err(std::io::Error::other(
            "handoff path component is not a directory",
        ));
    }
    Ok(())
}

fn ensure_private_dir_owned_0700(path: &Path) -> std::io::Result<()> {
    reject_symlink_dir(path)?;
    let meta = std::fs::metadata(path)?;
    let owner = meta.uid();
    let current = unsafe { libc::getuid() };
    if owner != current {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "private handoff directory must be owned by current user: {}",
                path.display()
            ),
        ));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn strip_handoff_environment(command: &mut Command) {
    for key in HANDOFF_ENV_KEYS {
        command.env_remove(key);
    }
}

pub fn runtime_handoff_aish_root(
    xdg_runtime_dir: Option<&std::ffi::OsStr>,
    tmpdir: Option<&std::ffi::OsStr>,
    uid: u32,
) -> PathBuf {
    if let Some(dir) = xdg_runtime_dir {
        PathBuf::from(dir).join("aish")
    } else if let Some(dir) = tmpdir {
        PathBuf::from(dir).join("aish")
    } else {
        PathBuf::from(format!("/tmp/aish-{uid}"))
    }
}

pub fn create_runtime_handoff_dir_from(
    xdg_runtime_dir: Option<&std::ffi::OsStr>,
    tmpdir: Option<&std::ffi::OsStr>,
    uid: u32,
) -> std::io::Result<PathBuf> {
    let aish_root = runtime_handoff_aish_root(xdg_runtime_dir, tmpdir, uid);
    ensure_dir_0700(&aish_root)?;
    ensure_private_dir_owned_0700(&aish_root)?;
    let dir = aish_root.join(format!(
        "handoff-{}-{}",
        std::process::id(),
        rand_handoff_suffix()
    ));
    ensure_dir_0700(&dir)?;
    ensure_private_dir_owned_0700(&dir)?;
    Ok(dir)
}

pub fn create_runtime_handoff_dir() -> std::io::Result<PathBuf> {
    create_runtime_handoff_dir_from(
        std::env::var_os("XDG_RUNTIME_DIR").as_deref(),
        std::env::var_os("TMPDIR").as_deref(),
        unsafe { libc::getuid() },
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn handoff_env_must_be_set_after_strip() {
        let mut wrong_order = Command::new("sh");
        wrong_order
            .arg("-c")
            .arg("printf %s \"$AISH_CONTROL_MODE\"");
        wrong_order.env("AISH_CONTROL_MODE", "human-shell");
        strip_handoff_environment(&mut wrong_order);
        let wrong = wrong_order.output().expect("sh");
        assert!(
            String::from_utf8_lossy(&wrong.stdout).is_empty(),
            "env_remove after env clears the value (regression guard)"
        );

        let mut correct_order = Command::new("sh");
        correct_order
            .arg("-c")
            .arg("printf %s \"$AISH_CONTROL_MODE\"");
        strip_handoff_environment(&mut correct_order);
        correct_order.env("AISH_CONTROL_MODE", "human-shell");
        let ok = correct_order.output().expect("sh");
        assert_eq!(
            String::from_utf8_lossy(&ok.stdout),
            "human-shell",
            "strip inherited handoff env before setting child values"
        );
    }

    #[test]
    fn runtime_handoff_dir_tightens_owned_insecure_aish_root() {
        let base = tempfile::tempdir().unwrap();
        let parent = base.path().join("runtime");
        std::fs::create_dir_all(&parent).unwrap();
        let aish_root = parent.join("aish");
        std::fs::create_dir_all(&aish_root).unwrap();
        let mut perms = std::fs::metadata(&aish_root).unwrap().permissions();
        perms.set_mode(0o775);
        std::fs::set_permissions(&aish_root, perms).unwrap();
        let parent_before = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
        std::env::set_var("XDG_RUNTIME_DIR", &parent);
        let handoff = create_runtime_handoff_dir().expect("tighten owned aish root");
        let aish_mode = std::fs::metadata(&aish_root).unwrap().permissions().mode() & 0o777;
        assert_eq!(aish_mode, 0o700);
        let handoff_mode = std::fs::metadata(&handoff).unwrap().permissions().mode() & 0o777;
        assert_eq!(handoff_mode, 0o700);
        let parent_after = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
        assert_eq!(parent_before, parent_after);
    }

    #[test]
    fn runtime_handoff_dir_does_not_chmod_existing_parent() {
        let base = tempfile::tempdir().unwrap();
        let parent = base.path().join("shared-runtime");
        std::fs::create_dir_all(&parent).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&parent).unwrap().permissions();
            perms.set_mode(0o1777);
            std::fs::set_permissions(&parent, perms).unwrap();
            let before = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
            std::env::set_var("XDG_RUNTIME_DIR", &parent);
            let handoff = create_runtime_handoff_dir().expect("runtime dir");
            let after = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                before, after,
                "existing runtime parent must not be chmodded"
            );
            let aish_root = parent.join("aish");
            let aish_mode = std::fs::metadata(&aish_root).unwrap().permissions().mode() & 0o777;
            assert_eq!(aish_mode, 0o700);
            let handoff_mode = std::fs::metadata(&handoff).unwrap().permissions().mode() & 0o777;
            assert_eq!(handoff_mode, 0o700);
        }
    }

    #[test]
    fn runtime_handoff_dir_is_0700() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        let handoff = create_runtime_handoff_dir().expect("runtime dir");
        let mode = std::fs::metadata(&handoff).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let aish_root = dir.path().join("aish");
        let root_mode = std::fs::metadata(&aish_root).unwrap().permissions().mode() & 0o777;
        assert_eq!(root_mode, 0o700);
    }

    #[test]
    fn runtime_handoff_dir_fallback_uses_uid() {
        let uid = unsafe { libc::getuid() };
        let expected_root = runtime_handoff_aish_root(None, None, uid);
        let handoff = create_runtime_handoff_dir_from(None, None, uid).expect("runtime dir");
        assert!(
            handoff.starts_with(&expected_root),
            "handoff={} expected_root={}",
            handoff.display(),
            expected_root.display()
        );
        cleanup_runtime_handoff_dir(&handoff);
    }

    #[test]
    fn shell_log_tail_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session");
        std::fs::create_dir_all(&session).unwrap();
        let log = session.join("log.jsonl");
        let huge = "x".repeat(SHELL_LOG_TAIL_MAX_BYTES + 4096);
        std::fs::write(&log, huge.as_bytes()).unwrap();
        let reader = FileShellTranscriptReader;
        let (tail, truncated) = reader.read_tail(&session, 0, None).expect("tail");
        assert!(truncated);
        assert!(tail.len() <= SHELL_LOG_TAIL_MAX_BYTES + 16);
    }
}
