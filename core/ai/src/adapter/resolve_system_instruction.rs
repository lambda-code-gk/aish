//! 有効な sysq を結合して system instruction を返す標準アダプタ

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use common::adapter::{FileSystem, EnvResolver};
use common::error::Error;
use common::system_prompt::merge_enabled_ordered;

use crate::ports::outbound::ResolveSystemInstruction;

const ENABLED_FILENAME: &str = "enabled";

/// 標準実装（EnvResolver + FileSystem）
pub struct StdResolveSystemInstruction {
    env: std::sync::Arc<dyn EnvResolver>,
    fs: std::sync::Arc<dyn FileSystem>,
}

impl StdResolveSystemInstruction {
    pub fn new(
        env: std::sync::Arc<dyn EnvResolver>,
        fs: std::sync::Arc<dyn FileSystem>,
    ) -> Self {
        Self { env, fs }
    }
}

impl ResolveSystemInstruction for StdResolveSystemInstruction {
    fn resolve(&self) -> Result<Option<String>, Error> {
        let global_dir = self.env.resolve_global_system_d_dir()?;
        let user_dir = self.env.resolve_user_system_d_dir()?;
        let project_dir = resolve_project_system_d(&*self.env, &*self.fs)?;

        let (g_available, g_enabled) = dir_to_maps(&*self.fs, global_dir.as_deref())?;
        let (u_available, u_enabled) = dir_to_maps(&*self.fs, user_dir.as_deref())?;
        let (p_available, p_enabled) = dir_to_maps(&*self.fs, project_dir.as_deref())?;

        let ordered = merge_enabled_ordered(
            &g_available,
            &g_enabled,
            &u_available,
            &u_enabled,
            &p_available,
            &p_enabled,
        );

        if ordered.is_empty() {
            return Ok(None);
        }

        let mut parts = Vec::with_capacity(ordered.len());
        for (_id, path) in ordered {
            let s = self.fs.read_to_string(&path)?;
            let s = s.trim();
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(parts.join("\n\n---\n\n")))
    }
}

fn resolve_project_system_d(
    env: &dyn EnvResolver,
    fs: &dyn FileSystem,
) -> Result<Option<PathBuf>, Error> {
    let mut current = env.current_dir()?;
    loop {
        let system_d = current.join(".aish").join("system.d");
        if fs.exists(&system_d) {
            return Ok(Some(system_d));
        }
        if !current.pop() {
            break;
        }
    }
    Ok(None)
}

fn dir_to_maps(
    fs: &dyn FileSystem,
    dir_opt: Option<&Path>,
) -> Result<(HashMap<String, PathBuf>, Vec<String>), Error> {
    let mut available = HashMap::new();
    let mut enabled = Vec::new();

    let dir = match dir_opt {
        Some(d) if fs.exists(d) => d,
        _ => return Ok((available, enabled)),
    };

    let enabled_path = dir.join(ENABLED_FILENAME);
    if fs.exists(&enabled_path) {
        let s = fs.read_to_string(&enabled_path)?;
        enabled = s
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
    }

    collect_id_paths(fs, dir, PathBuf::new(), &mut available)?;
    Ok((available, enabled))
}

fn collect_id_paths(
    fs: &dyn FileSystem,
    base: &Path,
    rel: PathBuf,
    out: &mut HashMap<String, PathBuf>,
) -> Result<(), Error> {
    let full = if rel.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        base.join(&rel)
    };
    let entries = fs.read_dir(&full)?;
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == ENABLED_FILENAME {
            continue;
        }
        if fs.metadata(&path).map(|m| m.is_dir()).unwrap_or(false) {
            let name = name.to_string();
            let next_rel = if rel.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                rel.join(&name)
            };
            collect_id_paths(fs, base, next_rel, out)?;
            continue;
        }
        let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name);
        let id = if rel.as_os_str().is_empty() {
            stem.to_string()
        } else {
            format!("{}/{}", rel.display(), stem)
        };
        out.insert(id, path);
    }
    Ok(())
}
