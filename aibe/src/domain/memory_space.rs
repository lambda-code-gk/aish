//! memory space ID の解決（domain）。

use aibe_protocol::{
    is_valid_memory_space_id, is_valid_session_id, legacy_session_memory_space_id,
    project_memory_space_id, MemoryContext,
};

use super::{MemoryValidationError, ProjectKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySpaceId(String);

impl MemorySpaceId {
    pub fn new(id: impl Into<String>) -> Result<Self, MemoryValidationError> {
        let id = id.into();
        if !is_valid_memory_space_id(&id) {
            return Err(MemoryValidationError::InvalidMemorySpaceId(id));
        }
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySpaceSource {
    Explicit,
    Project,
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySpaceResolution {
    pub id: MemorySpaceId,
    pub source: MemorySpaceSource,
}

/// request / project / legacy の優先順で `memory_space_id` を解決する。
/// `AIBE_CONTEXT_ID` はクライアント側の context selection のみ。サーバ環境変数は参照しない。
pub fn resolve_memory_space(
    session_id: &str,
    context: &MemoryContext,
    project_key: Option<&ProjectKey>,
) -> Result<MemorySpaceResolution, MemoryValidationError> {
    if !is_valid_session_id(session_id) {
        return Err(MemoryValidationError::InvalidSessionId(
            session_id.to_string(),
        ));
    }
    if let Some(id) = context.memory_space_id.as_deref().filter(|s| !s.is_empty()) {
        return Ok(MemorySpaceResolution {
            id: MemorySpaceId::new(id)?,
            source: MemorySpaceSource::Explicit,
        });
    }
    if let Some(pk) = project_key {
        let id = project_memory_space_id(pk.as_str());
        return Ok(MemorySpaceResolution {
            id: MemorySpaceId::new(id)?,
            source: MemorySpaceSource::Project,
        });
    }
    let id = legacy_session_memory_space_id(session_id);
    Ok(MemorySpaceResolution {
        id: MemorySpaceId::new(id)?,
        source: MemorySpaceSource::Legacy,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFreshness {
    Current,
    Stale,
}

pub fn now_freshness(entry_last_session_id: &str, current_session_id: &str) -> MemoryFreshness {
    if entry_last_session_id == current_session_id {
        MemoryFreshness::Current
    } else {
        MemoryFreshness::Stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(cwd: Option<&str>, memory_space_id: Option<&str>) -> MemoryContext {
        MemoryContext {
            cwd: cwd.map(str::to_string),
            memory_space_id: memory_space_id.map(str::to_string),
        }
    }

    #[test]
    fn explicit_beats_project() {
        let pk = ProjectKey::new("/proj").expect("pk");
        let res = resolve_memory_space("sess", &ctx(Some("/proj"), Some("ctx_a")), Some(&pk))
            .expect("resolve");
        assert_eq!(res.id.as_str(), "ctx_a");
        assert_eq!(res.source, MemorySpaceSource::Explicit);
    }

    #[test]
    fn project_backed_from_key() {
        let pk = ProjectKey::new("/proj").expect("pk");
        let res =
            resolve_memory_space("sess", &ctx(Some("/proj"), None), Some(&pk)).expect("resolve");
        assert_eq!(res.source, MemorySpaceSource::Project);
        assert!(res.id.as_str().starts_with("project_"));
    }

    #[test]
    fn legacy_fallback_without_project() {
        let res =
            resolve_memory_space("sess_001", &ctx(Some("/proj"), None), None).expect("resolve");
        assert_eq!(res.id.as_str(), "legacy_session_sess_001");
        assert_eq!(res.source, MemorySpaceSource::Legacy);
    }

    #[test]
    fn now_stale_when_session_differs() {
        assert_eq!(
            now_freshness("sess_001", "sess_002"),
            MemoryFreshness::Stale
        );
        assert_eq!(
            now_freshness("sess_001", "sess_001"),
            MemoryFreshness::Current
        );
    }
}
