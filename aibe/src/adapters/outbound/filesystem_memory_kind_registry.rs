//! `<AIBE_ROOT>/memory/kinds.toml` と space-local override の読み込み。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use aibe_protocol::is_valid_memory_space_id;

use crate::domain::{
    KindOverride, MemoryCardinality, MemoryInjectPolicy, MemoryKindRegistry,
    MemoryKindRegistryError, MemoryLifecycle, MemoryScope, MemoryStalePolicy, MemoryStatus,
    PromptOverride,
};
use crate::ports::outbound::MemoryKindRegistryLoader;

/// built-in のみ（filesystem override なし）。
#[derive(Debug, Default, Clone)]
pub struct BuiltinMemoryKindRegistryLoader;

impl MemoryKindRegistryLoader for BuiltinMemoryKindRegistryLoader {
    fn load_strict(
        &self,
        _memory_space_id: &str,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        Ok(MemoryKindRegistry::from_builtin())
    }

    fn load_best_effort(&self, _memory_space_id: &str) -> MemoryKindRegistry {
        MemoryKindRegistry::from_builtin()
    }
}

/// builtin + server + memory-space-local の merge。
#[derive(Debug, Clone)]
pub struct FilesystemMemoryKindRegistryLoader {
    aibe_root: PathBuf,
}

impl FilesystemMemoryKindRegistryLoader {
    pub fn new(aibe_root: PathBuf) -> Self {
        Self { aibe_root }
    }

    pub fn with_conversation_root(conversation_root: PathBuf) -> Self {
        let aibe_root = conversation_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(conversation_root);
        Self::new(aibe_root)
    }

    fn server_kinds_path(&self) -> PathBuf {
        self.aibe_root.join("memory").join("kinds.toml")
    }

    fn space_kinds_path(&self, memory_space_id: &str) -> PathBuf {
        self.aibe_root
            .join("memory")
            .join("spaces")
            .join(memory_space_id)
            .join("kinds.toml")
    }

    fn load_effective(
        &self,
        memory_space_id: &str,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        if !is_valid_memory_space_id(memory_space_id) {
            return Err(MemoryKindRegistryError::Parse(format!(
                "invalid memory_space_id: {memory_space_id}"
            )));
        }
        let mut registry = MemoryKindRegistry::from_builtin();
        let server_path = self.server_kinds_path();
        if server_path.is_file() {
            let overrides = parse_kinds_toml_file(&server_path)?;
            registry.merge_overrides(&overrides)?;
        }
        let space_path = self.space_kinds_path(memory_space_id);
        if space_path.is_file() {
            let overrides = parse_kinds_toml_file(&space_path)?;
            registry.merge_overrides(&overrides)?;
        }
        Ok(registry)
    }
}

impl MemoryKindRegistryLoader for FilesystemMemoryKindRegistryLoader {
    fn load_strict(
        &self,
        memory_space_id: &str,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        self.load_effective(memory_space_id)
    }

    fn load_best_effort(&self, memory_space_id: &str) -> MemoryKindRegistry {
        match self.load_effective(memory_space_id) {
            Ok(registry) => registry,
            Err(err) => {
                tracing::warn!(
                    memory_space_id,
                    error = %err,
                    "kind registry load failed; falling back to builtin for prompt resolve"
                );
                MemoryKindRegistry::from_builtin()
            }
        }
    }
}

pub fn shared_builtin_loader() -> Arc<dyn MemoryKindRegistryLoader> {
    Arc::new(BuiltinMemoryKindRegistryLoader)
}

#[derive(Debug, Deserialize)]
struct KindsTomlRoot {
    #[serde(default)]
    kinds: HashMap<String, KindTomlEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct KindTomlEntry {
    description: Option<String>,
    default_scope: Option<String>,
    default_inject: Option<String>,
    default_status: Option<String>,
    lifecycle: Option<String>,
    cardinality: Option<String>,
    clear_from: Option<String>,
    clear_to: Option<String>,
    stale: Option<String>,
    dedicated_cli: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    prompt: Option<PromptTomlEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct PromptTomlEntry {
    auto_inject: Option<bool>,
    on_demand: Option<bool>,
    priority: Option<u32>,
    #[serde(default)]
    keywords: Vec<String>,
    max_entries: Option<u32>,
}

fn parse_kinds_toml_file(
    path: &Path,
) -> Result<HashMap<String, KindOverride>, MemoryKindRegistryError> {
    let raw = fs::read_to_string(path)
        .map_err(|e| MemoryKindRegistryError::Io(format!("{}: {e}", path.display())))?;
    let root: KindsTomlRoot = toml::from_str(&raw)
        .map_err(|e| MemoryKindRegistryError::Parse(format!("{}: {e}", path.display())))?;
    let mut out = HashMap::new();
    for (id, entry) in root.kinds {
        out.insert(id, entry_to_override(&entry)?);
    }
    Ok(out)
}

fn entry_to_override(entry: &KindTomlEntry) -> Result<KindOverride, MemoryKindRegistryError> {
    let aliases = if entry.aliases.is_empty() {
        None
    } else {
        Some(entry.aliases.clone())
    };
    let prompt = entry.prompt.as_ref().map(|p| {
        let keywords = if p.keywords.is_empty() {
            None
        } else {
            Some(p.keywords.clone())
        };
        PromptOverride {
            auto_inject: p.auto_inject,
            on_demand: p.on_demand,
            priority: p.priority,
            keywords,
            max_entries: p.max_entries.map(Some),
        }
    });
    Ok(KindOverride {
        description: entry.description.clone(),
        default_scope: entry
            .default_scope
            .as_deref()
            .map(parse_scope)
            .transpose()?,
        default_inject: entry
            .default_inject
            .as_deref()
            .map(parse_inject)
            .transpose()?,
        default_status: entry
            .default_status
            .as_deref()
            .map(parse_status)
            .transpose()?,
        lifecycle: entry
            .lifecycle
            .as_deref()
            .map(parse_lifecycle)
            .transpose()?,
        cardinality: entry
            .cardinality
            .as_deref()
            .map(parse_cardinality)
            .transpose()?,
        clear_from: entry.clear_from.as_deref().map(parse_status).transpose()?,
        clear_to: entry.clear_to.as_deref().map(parse_status).transpose()?,
        prompt,
        stale: entry.stale.as_deref().map(parse_stale).transpose()?,
        dedicated_cli: entry.dedicated_cli.as_ref().map(|s| Some(s.clone())),
        aliases,
    })
}

fn parse_scope(raw: &str) -> Result<MemoryScope, MemoryKindRegistryError> {
    match raw {
        "session" => Ok(MemoryScope::Session),
        "project" => Ok(MemoryScope::Project),
        "global" => Ok(MemoryScope::Global),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown default_scope: {raw}"
        ))),
    }
}

fn parse_inject(raw: &str) -> Result<MemoryInjectPolicy, MemoryKindRegistryError> {
    match raw {
        "pinned" => Ok(MemoryInjectPolicy::Pinned),
        "on_demand" => Ok(MemoryInjectPolicy::OnDemand),
        "manual" => Ok(MemoryInjectPolicy::Manual),
        "never" => Ok(MemoryInjectPolicy::Never),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown default_inject: {raw}"
        ))),
    }
}

fn parse_status(raw: &str) -> Result<MemoryStatus, MemoryKindRegistryError> {
    match raw {
        "active" => Ok(MemoryStatus::Active),
        "inactive" => Ok(MemoryStatus::Inactive),
        "open" => Ok(MemoryStatus::Open),
        "archived" => Ok(MemoryStatus::Archived),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown status: {raw}"
        ))),
    }
}

fn parse_lifecycle(raw: &str) -> Result<MemoryLifecycle, MemoryKindRegistryError> {
    match raw {
        "active_inactive" => Ok(MemoryLifecycle::ActiveInactive),
        "open_archive" => Ok(MemoryLifecycle::OpenArchive),
        "active_archive" => Ok(MemoryLifecycle::ActiveArchive),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown lifecycle: {raw}"
        ))),
    }
}

fn parse_cardinality(raw: &str) -> Result<MemoryCardinality, MemoryKindRegistryError> {
    match raw {
        "single_effective" => Ok(MemoryCardinality::SingleEffective),
        "multiple" => Ok(MemoryCardinality::Multiple),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown cardinality: {raw}"
        ))),
    }
}

fn parse_stale(raw: &str) -> Result<MemoryStalePolicy, MemoryKindRegistryError> {
    match raw {
        "none" => Ok(MemoryStalePolicy::None),
        "session_changed" => Ok(MemoryStalePolicy::SessionChanged),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown stale: {raw}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_kinds(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        let mut f = fs::File::create(path).expect("create");
        f.write_all(body.as_bytes()).expect("write");
    }

    #[test]
    fn server_override_merges_aliases() {
        let dir = TempDir::new().expect("tempdir");
        let aibe_root = dir.path().to_path_buf();
        write_kinds(
            &aibe_root.join("memory/kinds.toml"),
            r#"
[kinds.goal]
description = "team goal"
aliases = ["goal", "チーム目標"]
prompt.priority = 5
"#,
        );
        let loader = FilesystemMemoryKindRegistryLoader::new(aibe_root);
        let reg = loader.load_strict("ctx_a").expect("load");
        let def = reg.get("goal").expect("goal");
        assert_eq!(def.description, "team goal");
        assert_eq!(def.prompt.priority, 5);
    }

    #[test]
    fn space_local_overrides_server() {
        let dir = TempDir::new().expect("tempdir");
        let aibe_root = dir.path().to_path_buf();
        write_kinds(
            &aibe_root.join("memory/kinds.toml"),
            r#"
[kinds.goal]
description = "server goal"
"#,
        );
        write_kinds(
            &aibe_root.join("memory/spaces/ctx_a/kinds.toml"),
            r#"
[kinds.goal]
description = "space goal"
"#,
        );
        let loader = FilesystemMemoryKindRegistryLoader::new(aibe_root);
        let reg = loader.load_strict("ctx_a").expect("load");
        assert_eq!(reg.get("goal").unwrap().description, "space goal");
    }

    #[test]
    fn parse_error_is_strict() {
        let dir = TempDir::new().expect("tempdir");
        let aibe_root = dir.path().to_path_buf();
        write_kinds(
            &aibe_root.join("memory/kinds.toml"),
            r#"
[kinds.goal]
default_scope = "invalid_scope"
"#,
        );
        let loader = FilesystemMemoryKindRegistryLoader::new(aibe_root);
        assert!(loader.load_strict("ctx_a").is_err());
    }

    #[test]
    fn invalid_memory_space_id_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        let loader = FilesystemMemoryKindRegistryLoader::new(dir.path().to_path_buf());
        assert!(loader.load_strict("../escape").is_err());
    }

    #[test]
    fn best_effort_falls_back_to_builtin() {
        let dir = TempDir::new().expect("tempdir");
        let aibe_root = dir.path().to_path_buf();
        write_kinds(
            &aibe_root.join("memory/kinds.toml"),
            r#"
[kinds.goal]
default_scope = "invalid_scope"
"#,
        );
        let loader = FilesystemMemoryKindRegistryLoader::new(aibe_root);
        let reg = loader.load_best_effort("ctx_a");
        assert_eq!(reg.get("goal").unwrap().description, "作業の最終目的");
    }
}
