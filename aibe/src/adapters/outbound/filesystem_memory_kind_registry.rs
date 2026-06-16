//! `<AIBE_ROOT>/memory/kinds.toml` と space-local override、または `kind_files` の読み込み。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::{
    parse_kinds_toml_str, KindOverride, MemoryKindRegistry, MemoryKindRegistryError,
};
use crate::ports::outbound::{MemoryConfig, MemoryKindRegistryLoader};

/// baseline pack のみ（filesystem override なし）。
#[derive(Debug, Default, Clone)]
pub struct BaselineMemoryKindRegistryLoader;

impl MemoryKindRegistryLoader for BaselineMemoryKindRegistryLoader {
    fn load_strict(
        &self,
        _memory_space_id: &str,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        MemoryKindRegistry::baseline()
    }

    fn load_best_effort(&self, _memory_space_id: &str) -> MemoryKindRegistry {
        MemoryKindRegistry::baseline().unwrap_or_else(|_| MemoryKindRegistry::empty())
    }
}

/// `kind_files` または互換モード（baseline + server + space override）の merge。
#[derive(Debug, Clone)]
pub struct FilesystemMemoryKindRegistryLoader {
    aibe_root: PathBuf,
    memory_config: MemoryConfig,
}

impl FilesystemMemoryKindRegistryLoader {
    pub fn new(aibe_root: PathBuf) -> Self {
        Self::with_memory_config(aibe_root, MemoryConfig::default())
    }

    pub fn with_memory_config(aibe_root: PathBuf, memory_config: MemoryConfig) -> Self {
        Self {
            aibe_root,
            memory_config,
        }
    }

    pub fn with_conversation_root(conversation_root: PathBuf) -> Self {
        let aibe_root = conversation_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(conversation_root);
        Self::new(aibe_root)
    }

    pub fn with_conversation_root_and_config(
        conversation_root: PathBuf,
        memory_config: MemoryConfig,
    ) -> Self {
        let aibe_root = conversation_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(conversation_root);
        Self::with_memory_config(aibe_root, memory_config)
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

    fn load_kind_file(
        path: &Path,
        mark_builtin: bool,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        let raw = fs::read_to_string(path)
            .map_err(|e| MemoryKindRegistryError::Io(format!("{}: {e}", path.display())))?;
        MemoryKindRegistry::load_from_str(&raw, &path.display().to_string(), mark_builtin)
    }

    fn load_overlay_file(
        path: &Path,
    ) -> Result<HashMap<String, KindOverride>, MemoryKindRegistryError> {
        let raw = fs::read_to_string(path)
            .map_err(|e| MemoryKindRegistryError::Io(format!("{}: {e}", path.display())))?;
        parse_kinds_toml_str(&raw, &path.display().to_string())
    }

    fn load_explicit_files(
        &self,
        files: &[PathBuf],
        best_effort: bool,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        let mut reg = MemoryKindRegistry::empty();
        for path in files {
            match Self::load_kind_file(path, false) {
                Ok(loaded) => reg.merge(loaded),
                Err(err) if best_effort => {
                    tracing::warn!(path = %path.display(), error = %err, "skipping broken kind pack file");
                }
                Err(err) => return Err(err),
            }
        }
        Ok(reg)
    }

    fn load_compat_mode(
        &self,
        memory_space_id: &str,
        best_effort: bool,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        use aibe_protocol::is_valid_memory_space_id;
        if !is_valid_memory_space_id(memory_space_id) {
            return Err(MemoryKindRegistryError::Parse(format!(
                "invalid memory_space_id: {memory_space_id}"
            )));
        }

        let mut registry = MemoryKindRegistry::baseline()?;
        let server_path = self.server_kinds_path();
        if server_path.is_file() {
            match Self::load_overlay_file(&server_path) {
                Ok(overrides) => registry.merge_overrides(&overrides)?,
                Err(err) if best_effort => {
                    tracing::warn!(path = %server_path.display(), error = %err, "skipping broken server kind overlay");
                }
                Err(err) => return Err(err),
            }
        }

        let space_path = self.space_kinds_path(memory_space_id);
        if space_path.is_file() {
            match Self::load_overlay_file(&space_path) {
                Ok(overrides) => registry.merge_overrides(&overrides)?,
                Err(err) if best_effort => {
                    tracing::warn!(path = %space_path.display(), error = %err, "skipping broken space kind overlay");
                }
                Err(err) => return Err(err),
            }
        }
        Ok(registry)
    }

    fn load_effective(
        &self,
        memory_space_id: &str,
        best_effort: bool,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        match &self.memory_config.kind_files {
            None => self.load_compat_mode(memory_space_id, best_effort),
            Some(files) if files.is_empty() => Ok(MemoryKindRegistry::empty()),
            Some(files) => self.load_explicit_files(files, best_effort),
        }
    }
}

impl MemoryKindRegistryLoader for FilesystemMemoryKindRegistryLoader {
    fn load_strict(
        &self,
        memory_space_id: &str,
    ) -> Result<MemoryKindRegistry, MemoryKindRegistryError> {
        self.load_effective(memory_space_id, false)
    }

    fn load_best_effort(&self, memory_space_id: &str) -> MemoryKindRegistry {
        self.load_effective(memory_space_id, true)
            .unwrap_or_else(|err| {
                tracing::warn!(
                    memory_space_id,
                    error = %err,
                    "kind registry load failed; falling back to baseline for prompt resolve"
                );
                MemoryKindRegistry::baseline().unwrap_or_else(|_| MemoryKindRegistry::empty())
            })
    }
}

pub fn shared_baseline_loader() -> Arc<dyn MemoryKindRegistryLoader> {
    Arc::new(BaselineMemoryKindRegistryLoader)
}

/// 後方互換 alias。
pub fn shared_builtin_loader() -> Arc<dyn MemoryKindRegistryLoader> {
    shared_baseline_loader()
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
    fn compat_server_override_merges_aliases() {
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
    fn best_effort_falls_back_to_baseline() {
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

    #[test]
    fn explicit_empty_kind_files_yields_no_kinds() {
        let dir = TempDir::new().expect("tempdir");
        let loader = FilesystemMemoryKindRegistryLoader::with_memory_config(
            dir.path().to_path_buf(),
            MemoryConfig {
                enabled: true,
                kind_files: Some(vec![]),
                recipe_files: None,
                feature_files: None,
            },
        );
        let reg = loader.load_strict("ctx_a").expect("load");
        assert!(reg.get("goal").is_none());
    }

    #[test]
    fn explicit_kind_files_do_not_apply_compat_overrides() {
        let dir = TempDir::new().expect("tempdir");
        let aibe_root = dir.path().to_path_buf();
        write_kinds(
            &aibe_root.join("memory/kinds.toml"),
            r#"
[kinds.goal]
description = "server goal"
"#,
        );
        let explicit = aibe_root.join("pack/kinds.toml");
        write_kinds(
            &explicit,
            r#"
[kinds.goal]
description = "pack goal"
default_scope = "project"
default_inject = "pinned"
default_status = "active"
lifecycle = "active_inactive"
cardinality = "single_effective"
clear_from = "active"
clear_to = "inactive"
"#,
        );
        write_kinds(
            &aibe_root.join("memory/spaces/ctx_a/kinds.toml"),
            r#"
[kinds.goal]
description = "space goal"
# invalid if merged from compat path; should be ignored
"#,
        );
        let loader = FilesystemMemoryKindRegistryLoader::with_memory_config(
            aibe_root,
            MemoryConfig {
                enabled: true,
                kind_files: Some(vec![explicit]),
                recipe_files: None,
                feature_files: None,
            },
        );
        let reg = loader.load_strict("ctx_a").expect("load");
        assert_eq!(reg.get("goal").unwrap().description, "pack goal");
    }

    #[test]
    fn best_effort_skips_broken_kind_file() {
        let dir = TempDir::new().expect("tempdir");
        let aibe_root = dir.path().to_path_buf();
        let good = aibe_root.join("pack/good.toml");
        write_kinds(
            &good,
            r#"
[kinds.goal]
description = "pack goal"
default_scope = "project"
default_inject = "pinned"
default_status = "active"
lifecycle = "active_inactive"
cardinality = "single_effective"
clear_from = "active"
clear_to = "inactive"
"#,
        );
        let bad = aibe_root.join("pack/bad.toml");
        write_kinds(&bad, "not toml [[[");
        let loader = FilesystemMemoryKindRegistryLoader::with_memory_config(
            aibe_root,
            MemoryConfig {
                enabled: true,
                kind_files: Some(vec![good, bad]),
                recipe_files: None,
                feature_files: None,
            },
        );
        let reg = loader.load_best_effort("ctx_a");
        assert_eq!(reg.get("goal").unwrap().description, "pack goal");
    }
}
