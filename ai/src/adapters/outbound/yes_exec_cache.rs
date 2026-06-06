//! `--yes-exec` 用の session 限定承認キャッシュ。

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use aibe_client::ShellExecApprovalPrompt;

#[derive(Debug, Clone)]
pub struct YesExecCache {
    path: PathBuf,
    keys: HashSet<String>,
}

impl YesExecCache {
    pub fn load(root: &Path, session_id: Option<&str>) -> anyhow::Result<Self> {
        let path = cache_path(root, session_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| anyhow::anyhow!(e))?;
        }
        let keys = if path.exists() {
            let raw = fs::read_to_string(&path).map_err(|e| anyhow::anyhow!(e))?;
            serde_json::from_str::<Vec<String>>(raw.trim())
                .map_err(|e| anyhow::anyhow!(e))?
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        };
        Ok(Self { path, keys })
    }

    pub fn should_auto_approve(&self, prompt: &ShellExecApprovalPrompt) -> bool {
        self.keys.contains(&approval_key(prompt))
    }

    pub fn remember(&mut self, prompt: &ShellExecApprovalPrompt) -> anyhow::Result<()> {
        self.keys.insert(approval_key(prompt));
        self.persist()
    }

    fn persist(&self) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| anyhow::anyhow!(e))?;
        let payload = serde_json::to_string(&self.keys).map_err(|e| anyhow::anyhow!(e))?;
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

fn approval_key(prompt: &ShellExecApprovalPrompt) -> String {
    format!(
        "{}\n{}",
        prompt.command,
        serde_json::to_string(&prompt.args).unwrap_or_default()
    )
}

fn cache_path(root: &Path, session_id: Option<&str>) -> PathBuf {
    match session_id.filter(|s| !s.is_empty()) {
        Some(session_id) => root.join("yes-exec").join(format!("{session_id}.json")),
        None => root.join("yes-exec").join("global.json"),
    }
}
