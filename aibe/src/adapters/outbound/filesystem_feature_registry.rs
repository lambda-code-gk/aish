//! `feature_files` または baseline pack の smart feature 読み込み。

use std::fs;
use std::path::Path;

use crate::domain::{FeatureRegistry, FeatureRegistryError};
use crate::ports::outbound::{FeatureRegistryLoader, MemoryConfig};

#[derive(Debug, Clone)]
pub struct FilesystemFeatureRegistryLoader {
    memory_config: MemoryConfig,
}

impl FilesystemFeatureRegistryLoader {
    pub fn new(memory_config: MemoryConfig) -> Self {
        Self { memory_config }
    }

    fn load_file(path: &Path) -> Result<FeatureRegistry, FeatureRegistryError> {
        let raw = fs::read_to_string(path)
            .map_err(|e| FeatureRegistryError::Io(format!("{}: {e}", path.display())))?;
        FeatureRegistry::load_from_str(&raw, &path.display().to_string())
    }
}

impl FeatureRegistryLoader for FilesystemFeatureRegistryLoader {
    fn load(&self) -> Result<FeatureRegistry, FeatureRegistryError> {
        match &self.memory_config.feature_files {
            None if self.memory_config.is_explicit_generic_memory_pack() => {
                Ok(FeatureRegistry::empty())
            }
            None => FeatureRegistry::baseline(),
            Some(files) if files.is_empty() => Ok(FeatureRegistry::empty()),
            Some(files) => {
                let mut merged = FeatureRegistry::empty();
                for path in files {
                    merged.merge(Self::load_file(path)?);
                }
                Ok(merged)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_files_none_loads_baseline_pack() {
        let loader = FilesystemFeatureRegistryLoader::new(MemoryConfig::default());
        let registry = loader.load().expect("load");
        assert!(!registry.feature_ids().is_empty());
        assert!(registry.feature_ids().contains(&"inspect_error"));
    }

    #[test]
    fn feature_files_empty_yields_empty_registry() {
        let loader = FilesystemFeatureRegistryLoader::new(MemoryConfig {
            enabled: true,
            kind_files: None,
            recipe_files: None,
            feature_files: Some(vec![]),
        });
        let registry = loader.load().expect("load");
        assert!(registry.feature_ids().is_empty());
    }

    #[test]
    fn generic_memory_pack_without_feature_files_yields_empty_registry() {
        let loader = FilesystemFeatureRegistryLoader::new(MemoryConfig {
            enabled: true,
            kind_files: Some(vec![]),
            recipe_files: Some(vec![]),
            feature_files: None,
        });
        let registry = loader.load().expect("load");
        assert!(registry.feature_ids().is_empty());
    }
}
