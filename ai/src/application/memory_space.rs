//! memory space ID のクライアント側解決（env / cwd は composition root から渡す）。

use std::path::Path;

use aibe_protocol::{
    is_valid_memory_space_id, is_valid_session_id, legacy_session_memory_space_id,
    project_memory_space_id, MemoryContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySpaceResolution {
    pub memory_space_id: String,
    pub source: MemorySpaceSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySpaceSource {
    Env,
    Config,
    Project,
    Legacy,
}

pub fn resolve_memory_space_id(
    session_id: &str,
    canonical_project_key: Option<&str>,
    config_current: Option<&str>,
    env_context_id: Option<&str>,
) -> Result<MemorySpaceResolution, String> {
    if !is_valid_session_id(session_id) {
        return Err("invalid session_id".into());
    }
    if let Some(id) = env_context_id.filter(|s| !s.is_empty()) {
        validate_id(id)?;
        return Ok(MemorySpaceResolution {
            memory_space_id: id.to_string(),
            source: MemorySpaceSource::Env,
        });
    }
    if let Some(id) = config_current.filter(|s| !s.is_empty()) {
        validate_id(id)?;
        return Ok(MemorySpaceResolution {
            memory_space_id: id.to_string(),
            source: MemorySpaceSource::Config,
        });
    }
    if let Some(pk) = canonical_project_key.filter(|s| !s.is_empty()) {
        let id = project_memory_space_id(pk);
        validate_id(&id)?;
        return Ok(MemorySpaceResolution {
            memory_space_id: id,
            source: MemorySpaceSource::Project,
        });
    }
    let id = legacy_session_memory_space_id(session_id);
    validate_id(&id)?;
    Ok(MemorySpaceResolution {
        memory_space_id: id,
        source: MemorySpaceSource::Legacy,
    })
}

pub fn build_memory_context(
    session_id: &str,
    canonical_cwd: &Path,
    canonical_project_key: Option<&str>,
    config_current: Option<&str>,
    env_context_id: Option<&str>,
) -> Result<MemoryContext, String> {
    let resolution = resolve_memory_space_id(
        session_id,
        canonical_project_key,
        config_current,
        env_context_id,
    )?;
    Ok(MemoryContext {
        cwd: Some(canonical_cwd.to_string_lossy().into_owned()),
        memory_space_id: Some(resolution.memory_space_id),
    })
}

fn validate_id(id: &str) -> Result<(), String> {
    if is_valid_memory_space_id(id) {
        Ok(())
    } else {
        Err(format!("invalid memory_space_id: {id}"))
    }
}

pub fn format_resolution(resolution: &MemorySpaceResolution) -> String {
    let source = match resolution.source {
        MemorySpaceSource::Env => "AIBE_CONTEXT_ID",
        MemorySpaceSource::Config => "config",
        MemorySpaceSource::Project => "project",
        MemorySpaceSource::Legacy => "legacy_session (deprecated)",
    };
    format!(
        "memory_space_id: {}\nsource: {source}",
        resolution.memory_space_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_beats_config_when_passed() {
        let res =
            resolve_memory_space_id("sess", Some("/tmp/proj"), Some("ctx_cfg"), Some("ctx_env"))
                .expect("resolve");
        assert_eq!(res.memory_space_id, "ctx_env");
        assert_eq!(res.source, MemorySpaceSource::Env);
    }

    #[test]
    fn config_beats_project() {
        let res = resolve_memory_space_id("sess", Some("/tmp/proj"), Some("ctx_cfg"), None)
            .expect("resolve");
        assert_eq!(res.memory_space_id, "ctx_cfg");
        assert_eq!(res.source, MemorySpaceSource::Config);
    }

    #[test]
    fn falls_back_to_legacy_without_project() {
        let res = resolve_memory_space_id("sess_001", None, None, None).expect("resolve");
        assert_eq!(res.memory_space_id, "legacy_session_sess_001");
        assert_eq!(res.source, MemorySpaceSource::Legacy);
    }
}
