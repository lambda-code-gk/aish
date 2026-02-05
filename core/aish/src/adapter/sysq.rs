//! システムプロンプト（sysq）の標準アダプタ
//!
//! EnvResolver で global/user/project の system.d を解決し、
//! FileSystem で一覧・enabled の読み書きを行う。

use std::path::{Path, PathBuf};

use common::adapter::FileSystem;
use common::error::Error;
use common::ports::outbound::EnvResolver;
use common::system_prompt::Scope;

use crate::ports::outbound::{SysqListEntry, SysqRepository};

const ENABLED_FILENAME: &str = "enabled";

/// 標準 sysq リポジトリ（EnvResolver + FileSystem）
pub struct StdSysqRepository {
    env: std::sync::Arc<dyn EnvResolver>,
    fs: std::sync::Arc<dyn FileSystem>,
}

impl StdSysqRepository {
    pub fn new(env: std::sync::Arc<dyn EnvResolver>, fs: std::sync::Arc<dyn FileSystem>) -> Self {
        Self { env, fs }
    }
}

impl SysqRepository for StdSysqRepository {
    fn list_entries(&self) -> Result<Vec<SysqListEntry>, Error> {
        let mut entries = Vec::new();

        for (scope, dir_opt) in [
            (Scope::Global, self.env.resolve_global_system_d_dir()?),
            (Scope::User, self.env.resolve_user_system_d_dir()?),
            (Scope::Project, resolve_project_system_d(&*self.env, &*self.fs)?),
        ] {
            if let Some(dir) = dir_opt {
                if !self.fs.exists(&dir) {
                    continue;
                }
                let enabled = read_enabled_ids(&*self.fs, &dir)?;
                collect_entries(&*self.fs, &dir, PathBuf::new(), scope, &enabled, &mut entries)?;
            }
        }

        Ok(entries)
    }

    fn enable(&self, ids: &[String]) -> Result<(), Error> {
        for id in ids {
            if let Some(dir) = find_scope_dir_for_id(self, id)? {
                add_enabled(&*self.fs, &dir, id)?;
            }
        }
        Ok(())
    }

    fn disable(&self, ids: &[String]) -> Result<(), Error> {
        for id in ids {
            if let Some(dir) = find_scope_dir_for_id(self, id)? {
                remove_enabled(&*self.fs, &dir, id)?;
            }
        }
        Ok(())
    }
}

fn resolve_project_system_d(
    env: &dyn EnvResolver,
    fs: &dyn FileSystem,
) -> Result<Option<PathBuf>, Error> {
    let mut current = env.current_dir()?;
    loop {
        let aish = current.join(".aish");
        let system_d = aish.join("system.d");
        if fs.exists(&system_d) {
            return Ok(Some(system_d));
        }
        if !current.pop() {
            break;
        }
    }
    Ok(None)
}

fn read_enabled_ids(fs: &dyn FileSystem, system_d_dir: &Path) -> Result<Vec<String>, Error> {
    let path = system_d_dir.join(ENABLED_FILENAME);
    if !fs.exists(&path) {
        return Ok(Vec::new());
    }
    let s = fs.read_to_string(&path)?;
    Ok(s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

fn collect_entries(
    fs: &dyn FileSystem,
    base: &Path,
    rel: PathBuf,
    scope: Scope,
    enabled: &[String],
    out: &mut Vec<SysqListEntry>,
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
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let mut next_rel = rel.clone();
            if !next_rel.as_os_str().is_empty() {
                next_rel.push(&name);
            } else {
                next_rel = PathBuf::from(&name);
            }
            collect_entries(fs, base, next_rel, scope, enabled, out)?;
            continue;
        }
        let stem = Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name);
        let id = if rel.as_os_str().is_empty() {
            stem.to_string()
        } else {
            format!("{}/{}", rel.display(), stem)
        };
        let enabled = enabled.contains(&id);
        let title = fs.read_to_string(&path).ok().and_then(|s| s.lines().next().map(String::from)).unwrap_or_default();
        out.push(SysqListEntry {
            id,
            scope,
            enabled,
            title,
        });
    }
    Ok(())
}

fn find_scope_dir_for_id(
    repo: &StdSysqRepository,
    id: &str,
) -> Result<Option<PathBuf>, Error> {
    let dirs = [
        resolve_project_system_d(&*repo.env, &*repo.fs)?,
        repo.env.resolve_user_system_d_dir()?,
        repo.env.resolve_global_system_d_dir()?,
    ];
    for dir_opt in dirs {
        if let Some(ref dir) = dir_opt {
            if repo.fs.exists(dir) && id_exists_in_dir(&*repo.fs, dir, id) {
                return Ok(Some(dir.clone()));
            }
        }
    }
    Ok(None)
}

fn id_exists_in_dir(fs: &dyn FileSystem, system_d_dir: &Path, id: &str) -> bool {
    let with_txt = system_d_dir.join(id).with_extension("txt");
    if fs.exists(&with_txt) {
        return true;
    }
    let no_ext = system_d_dir.join(id);
    fs.exists(&no_ext) && fs.metadata(&no_ext).map(|m| m.is_file()).unwrap_or(false)
}

fn add_enabled(fs: &dyn FileSystem, system_d_dir: &Path, id: &str) -> Result<(), Error> {
    let path = system_d_dir.join(ENABLED_FILENAME);
    let mut ids = read_enabled_ids(fs, system_d_dir)?;
    if !ids.contains(&id.to_string()) {
        ids.push(id.to_string());
    }
    let contents = ids.join("\n") + "\n";
    fs.write(&path, &contents)
}

fn remove_enabled(fs: &dyn FileSystem, system_d_dir: &Path, id: &str) -> Result<(), Error> {
    let path = system_d_dir.join(ENABLED_FILENAME);
    let mut ids = read_enabled_ids(fs, system_d_dir)?;
    ids.retain(|s| s != id);
    let contents = ids.join("\n") + if ids.is_empty() { "" } else { "\n" };
    fs.write(&path, &contents)
}
