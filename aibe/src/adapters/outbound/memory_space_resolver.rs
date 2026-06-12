//! memory space 解決（env / cwd I/O）。

use std::path::{Path, PathBuf};

use aibe_protocol::MemoryContext;

use crate::domain::{resolve_memory_space, ProjectKey, ProjectKeyError};
use crate::ports::outbound::{ContextualMemoryStoreError, MemorySpaceResolver, MemoryStoreContext};

#[derive(Debug, Default, Clone)]
pub struct FilesystemMemorySpaceResolver;

impl MemorySpaceResolver for FilesystemMemorySpaceResolver {
    fn resolve_store_context<'a>(
        &self,
        session_id: &'a str,
        context: &MemoryContext,
        cwd_path: &'a Path,
    ) -> Result<MemoryStoreContext<'a>, ContextualMemoryStoreError> {
        let project_key = project_key_from_cwd(cwd_path)?;
        let env_context = std::env::var("AIBE_CONTEXT_ID").ok();
        let resolution = resolve_memory_space(
            session_id,
            context,
            env_context.as_deref(),
            project_key.as_ref(),
        )
        .map_err(ContextualMemoryStoreError::Validation)?;
        Ok(MemoryStoreContext {
            session_id,
            memory_space_id: resolution.id.as_str().to_string(),
            cwd: Some(cwd_path),
        })
    }

    fn resolve_for_turn<'a>(
        &self,
        session_id: &'a str,
        explicit_memory_space_id: Option<&str>,
        cwd_path: Option<&'a Path>,
    ) -> Result<MemoryStoreContext<'a>, ContextualMemoryStoreError> {
        let mem_ctx = MemoryContext {
            cwd: cwd_path
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            memory_space_id: explicit_memory_space_id.map(str::to_string),
        };
        let env_context = std::env::var("AIBE_CONTEXT_ID").ok();
        // 注入は best-effort: project key 解決の失敗はエラーにせず legacy fallback に落とす。
        let project_key = cwd_path.and_then(|cwd| project_key_from_cwd(cwd).ok().flatten());
        let resolution = resolve_memory_space(
            session_id,
            &mem_ctx,
            env_context.as_deref(),
            project_key.as_ref(),
        )
        .map_err(ContextualMemoryStoreError::Validation)?;
        Ok(MemoryStoreContext {
            session_id,
            memory_space_id: resolution.id.as_str().to_string(),
            cwd: cwd_path,
        })
    }
}

fn project_key_from_cwd(cwd: &Path) -> Result<Option<ProjectKey>, ContextualMemoryStoreError> {
    let abs = cwd.canonicalize().map_err(|e| {
        ContextualMemoryStoreError::ProjectKey(ProjectKeyError::Resolve(e.to_string()))
    })?;
    let key = find_git_root(&abs).unwrap_or(abs);
    let canonical = key.canonicalize().map_err(|e| {
        ContextualMemoryStoreError::ProjectKey(ProjectKeyError::Resolve(e.to_string()))
    })?;
    ProjectKey::new(canonical.to_string_lossy())
        .map(Some)
        .map_err(ContextualMemoryStoreError::ProjectKey)
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
