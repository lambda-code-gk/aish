//! メモリ用ディレクトリ解決の標準実装
//!
//! カレントから上に遡って .aish/memory を探し、無ければグローバル（EnvResolver::resolve_dirs().data_dir/memory）を返す。

use crate::ports::outbound::ResolveMemoryDir;
use common::error::Error;
use common::ports::outbound::EnvResolver;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MEMORY_SUBDIR: &str = "memory";
const AISH_DIR: &str = ".aish";

pub struct StdResolveMemoryDir {
    env: Arc<dyn EnvResolver>,
}

impl StdResolveMemoryDir {
    pub fn new(env: Arc<dyn EnvResolver>) -> Self {
        Self { env }
    }
}

impl ResolveMemoryDir for StdResolveMemoryDir {
    fn resolve(&self) -> Result<(Option<PathBuf>, PathBuf), Error> {
        // ディレクトリ解決は EnvResolver::resolve_dirs() に集約し、home を data/config の「root」として扱わない
        let dirs = self.env.resolve_dirs()?;
        let global = dirs.data_dir.join(MEMORY_SUBDIR);

        let project = find_project_memory_dir(self.env.current_dir()?.as_path())?;
        Ok((project, global))
    }
}

/// カレントから上に遡り、.aish/memory が存在するディレクトリを返す。無ければ None。
fn find_project_memory_dir(mut current: &Path) -> Result<Option<PathBuf>, Error> {
    loop {
        let candidate = current.join(AISH_DIR).join(MEMORY_SUBDIR);
        if candidate.exists() {
            let meta = std::fs::metadata(&candidate).map_err(|e| {
                Error::io_msg(format!("metadata {}: {}", candidate.display(), e))
            })?;
            if meta.is_dir() {
                return Ok(Some(candidate));
            }
        }
        match current.parent() {
            Some(p) => current = p,
            None => return Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_project_memory_dir_no_aish_returns_none() {
        let tmp = std::env::temp_dir().join("memory_dir_test_none");
        let _ = std::fs::create_dir_all(&tmp);
        let r = find_project_memory_dir(tmp.as_path()).unwrap();
        assert!(r.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_project_memory_dir_finds_aish_memory() {
        let tmp = std::env::temp_dir().join("memory_dir_test_find");
        let _ = std::fs::create_dir_all(tmp.join(".aish").join("memory"));
        let r = find_project_memory_dir(tmp.as_path()).unwrap();
        assert!(r.is_some());
        assert!(r.unwrap().ends_with(".aish/memory"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
