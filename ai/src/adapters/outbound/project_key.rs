//! cwd から project_key を導出（adapter I/O）。

use std::path::{Path, PathBuf};

pub fn canonical_project_key_from_cwd(cwd: &Path) -> Result<Option<String>, String> {
    let abs = cwd
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize cwd: {e}"))?;
    let key = find_git_root(&abs).unwrap_or(abs);
    let canonical = key
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize project key: {e}"))?;
    Ok(Some(canonical.to_string_lossy().into_owned()))
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}
