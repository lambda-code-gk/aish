//! `recipe_files` または互換モード（baseline pack）の読み込み。

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::domain::{MemoryRecipeRegistry, MemoryRecipeRegistryError};
use crate::ports::outbound::{MemoryConfig, MemoryRecipeRegistryLoader};

#[derive(Debug, Clone)]
pub struct FilesystemMemoryRecipeRegistryLoader {
    memory_config: MemoryConfig,
}

impl FilesystemMemoryRecipeRegistryLoader {
    pub fn new(memory_config: MemoryConfig) -> Self {
        Self { memory_config }
    }

    fn load_recipe_file(path: &Path) -> Result<MemoryRecipeRegistry, MemoryRecipeRegistryError> {
        let raw = fs::read_to_string(path)
            .map_err(|e| MemoryRecipeRegistryError::Io(format!("{}: {e}", path.display())))?;
        let toml: toml::Table = toml::from_str(&raw)
            .map_err(|e| MemoryRecipeRegistryError::Parse(format!("{}: {e}", path.display())))?;
        let prompt_md = toml
            .get("prompt_md")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                MemoryRecipeRegistryError::Parse(format!("{}: missing prompt_md", path.display()))
            })?;
        let prompt_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(prompt_md);
        let system_prompt = fs::read_to_string(&prompt_path).map_err(|e| {
            MemoryRecipeRegistryError::Io(format!("{}: {e}", prompt_path.display()))
        })?;
        MemoryRecipeRegistry::load_from_str(&raw, &path.display().to_string(), &system_prompt)
    }

    fn load_explicit_files(
        files: &[std::path::PathBuf],
        best_effort: bool,
    ) -> Result<MemoryRecipeRegistry, MemoryRecipeRegistryError> {
        let mut registry = MemoryRecipeRegistry::empty();
        for path in files {
            match Self::load_recipe_file(path) {
                Ok(loaded) => registry.merge(loaded),
                Err(err) if best_effort => {
                    tracing::warn!(path = %path.display(), error = %err, "skipping broken recipe pack file");
                }
                Err(err) => return Err(err),
            }
        }
        Ok(registry)
    }

    fn load_effective(
        &self,
        best_effort: bool,
    ) -> Result<MemoryRecipeRegistry, MemoryRecipeRegistryError> {
        match &self.memory_config.recipe_files {
            None => MemoryRecipeRegistry::baseline(),
            Some(files) if files.is_empty() => Ok(MemoryRecipeRegistry::empty()),
            Some(files) => Self::load_explicit_files(files, best_effort),
        }
    }
}

impl MemoryRecipeRegistryLoader for FilesystemMemoryRecipeRegistryLoader {
    fn load_strict(&self) -> Result<MemoryRecipeRegistry, MemoryRecipeRegistryError> {
        self.load_effective(false)
    }

    fn load_best_effort(&self) -> MemoryRecipeRegistry {
        self.load_effective(true).unwrap_or_else(|err| {
            tracing::warn!(error = %err, "recipe registry load failed; falling back to baseline");
            MemoryRecipeRegistry::baseline().unwrap_or_else(|_| MemoryRecipeRegistry::empty())
        })
    }
}

pub fn shared_baseline_recipe_loader() -> Arc<dyn MemoryRecipeRegistryLoader> {
    Arc::new(FilesystemMemoryRecipeRegistryLoader::new(
        MemoryConfig::default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_recipe(path: &std::path::Path, body: &str, prompt: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path.with_extension("md"), prompt).expect("write md");
        let mut f = fs::File::create(path).expect("create");
        f.write_all(body.as_bytes()).expect("write");
    }

    #[test]
    fn explicit_empty_recipe_files_yields_no_recipes() {
        let loader = FilesystemMemoryRecipeRegistryLoader::new(MemoryConfig {
            enabled: true,
            kind_files: None,
            recipe_files: Some(vec![]),
            feature_files: None,
        });
        let reg = loader.load_strict().expect("load");
        assert!(reg.get("clarify-goal").is_none());
    }

    #[test]
    fn compat_mode_loads_clarify_goal() {
        let loader = FilesystemMemoryRecipeRegistryLoader::new(MemoryConfig::default());
        let reg = loader.load_strict().expect("load");
        assert!(reg.get("clarify-goal").is_some());
    }

    #[test]
    fn explicit_recipe_files_do_not_apply_compat_baseline() {
        let dir = TempDir::new().expect("tempdir");
        let good = dir.path().join("pack/clarify-goal.toml");
        write_recipe(
            &good,
            r#"
id = "clarify-goal"
description = "custom"
llm_profile = "default"
prompt_md = "clarify-goal.md"

[[materials]]
name = "goal_query"
title = "Active goal"
kind = "goal"
scope = "project"
status = "active"
limit = 1

[output]
format = "json"
summary_required = true
allow_operations = ["add"]
"#,
            "# custom prompt",
        );
        let loader = FilesystemMemoryRecipeRegistryLoader::new(MemoryConfig {
            enabled: true,
            kind_files: None,
            recipe_files: Some(vec![good]),
            feature_files: None,
        });
        let reg = loader.load_strict().expect("load");
        let def = reg.get("clarify-goal").expect("recipe");
        assert_eq!(def.description, "custom");
        assert_eq!(def.materials.len(), 1);
    }

    #[test]
    fn best_effort_skips_broken_recipe_file() {
        let dir = TempDir::new().expect("tempdir");
        let good = dir.path().join("pack/clarify-goal.toml");
        write_recipe(
            &good,
            r#"
id = "clarify-goal"
description = "custom"
llm_profile = "default"
prompt_md = "clarify-goal.md"

[[materials]]
name = "goal_query"
title = "Active goal"
kind = "goal"
scope = "project"
status = "active"
limit = 1

[output]
format = "json"
summary_required = true
allow_operations = ["add"]
"#,
            "# custom prompt",
        );
        let bad = dir.path().join("pack/bad.toml");
        fs::create_dir_all(bad.parent().unwrap()).expect("mkdir");
        fs::write(&bad, "not toml [[[ ").expect("write bad");
        let loader = FilesystemMemoryRecipeRegistryLoader::new(MemoryConfig {
            enabled: true,
            kind_files: None,
            recipe_files: Some(vec![good, bad]),
            feature_files: None,
        });
        let reg = loader.load_best_effort();
        let def = reg.get("clarify-goal").expect("recipe");
        assert_eq!(def.description, "custom");
    }
}
