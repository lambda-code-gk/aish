//! `--yes-exec` 用の session 限定承認キャッシュ。

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::domain::{
    command_shell_exec_key, exact_shell_exec_key, ShellExecApprovalChoice, ShellExecRememberScope,
    ShellExecTier,
};

#[derive(Debug, Clone)]
pub struct YesExecCache {
    path: PathBuf,
    exact_invocations: HashSet<String>,
    command_names: HashSet<String>,
}

impl YesExecCache {
    pub fn load(root: &Path, session_id: Option<&str>) -> anyhow::Result<Self> {
        let path = cache_path(root, session_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| anyhow::anyhow!(e))?;
        }
        if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| anyhow::anyhow!(e))?;
            if let Ok(payload) = serde_json::from_str::<YesExecCachePayload>(raw.trim()) {
                return Ok(Self {
                    path,
                    exact_invocations: payload.exact_invocations.into_iter().collect(),
                    command_names: payload.command_names.into_iter().collect(),
                });
            }
            if let Ok(legacy) = serde_json::from_str::<Vec<String>>(raw.trim()) {
                return Ok(Self {
                    path,
                    exact_invocations: legacy.into_iter().collect(),
                    command_names: HashSet::new(),
                });
            }
        }
        Ok(Self {
            path,
            exact_invocations: HashSet::new(),
            command_names: HashSet::new(),
        })
    }

    pub fn should_auto_approve(
        &self,
        command: &str,
        args: &[String],
        tier: ShellExecTier,
    ) -> Option<ShellExecRememberScope> {
        if tier == ShellExecTier::Destructive {
            return None;
        }
        let exact = exact_shell_exec_key(command, args);
        if self.exact_invocations.contains(&exact) {
            return Some(ShellExecRememberScope::ExactInvocation);
        }
        let legacy = legacy_exact_shell_exec_key(command, args);
        if self.exact_invocations.contains(&legacy) {
            return Some(ShellExecRememberScope::ExactInvocation);
        }
        let command_key = command_shell_exec_key(command, tier);
        if self.command_names.contains(&command_key) {
            return Some(ShellExecRememberScope::CommandName);
        }
        None
    }

    pub fn remember(
        &mut self,
        command: &str,
        args: &[String],
        tier: ShellExecTier,
        scope: ShellExecRememberScope,
    ) -> anyhow::Result<()> {
        if tier == ShellExecTier::Destructive {
            return Ok(());
        }
        match scope {
            ShellExecRememberScope::ExactInvocation => {
                self.exact_invocations
                    .insert(exact_shell_exec_key(command, args));
            }
            ShellExecRememberScope::CommandName => {
                self.command_names
                    .insert(command_shell_exec_key(command, tier));
            }
        }
        self.persist()
    }

    pub fn remember_choice(
        &mut self,
        command: &str,
        args: &[String],
        tier: ShellExecTier,
        choice: ShellExecApprovalChoice,
    ) -> anyhow::Result<()> {
        match choice {
            ShellExecApprovalChoice::Yes => Ok(()),
            ShellExecApprovalChoice::No => Ok(()),
            ShellExecApprovalChoice::AlwaysThisSession => {
                self.remember(command, args, tier, ShellExecRememberScope::ExactInvocation)
            }
            ShellExecApprovalChoice::CommandOnly => {
                self.remember(command, args, tier, ShellExecRememberScope::CommandName)
            }
        }
    }

    fn persist(&self) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| anyhow::anyhow!(e))?;
        let payload = serde_json::to_string(&YesExecCachePayload {
            exact_invocations: self.exact_invocations.iter().cloned().collect(),
            command_names: self.command_names.iter().cloned().collect(),
        })
        .map_err(|e| anyhow::anyhow!(e))?;
        writeln!(file, "{payload}").map_err(|e| anyhow::anyhow!(e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = file
                .metadata()
                .map_err(|e| anyhow::anyhow!(e))?
                .permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&self.path, perms).map_err(|e| anyhow::anyhow!(e))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct YesExecCachePayload {
    #[serde(default)]
    exact_invocations: Vec<String>,
    #[serde(default)]
    command_names: Vec<String>,
}

fn cache_path(root: &Path, session_id: Option<&str>) -> PathBuf {
    match session_id.filter(|s| !s.is_empty()) {
        Some(session_id) => root.join("yes-exec").join(format!("{session_id}.json")),
        None => root.join("yes-exec").join("global.json"),
    }
}

/// 0036 以前の `--yes-exec` cache キー（`command` + newline + args JSON）。
fn legacy_exact_shell_exec_key(command: &str, args: &[String]) -> String {
    format!(
        "{command}\n{}",
        serde_json::to_string(args).unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn legacy_cache_format_auto_approves() {
        let dir = tempdir().expect("tempdir");
        let cache_dir = dir.path().join("yes-exec");
        fs::create_dir_all(&cache_dir).expect("mkdir");
        let key = legacy_exact_shell_exec_key("echo", &["hi".into()]);
        fs::write(
            cache_dir.join("global.json"),
            serde_json::to_string(&vec![key]).expect("serialize"),
        )
        .expect("write");
        let cache = YesExecCache::load(dir.path(), None).expect("load");
        assert!(cache
            .should_auto_approve("echo", &["hi".into()], ShellExecTier::Mutating)
            .is_some());
    }
}
