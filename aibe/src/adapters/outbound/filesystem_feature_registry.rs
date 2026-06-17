//! `FeaturePackConfig` に基づく smart feature 読み込み（0043 Phase 3）。

use std::fs;
use std::path::Path;

use crate::domain::{
    EffectiveFeatureMode, FeaturePackResolution, FeatureRegistry, FeatureRegistryError,
};
use crate::ports::outbound::FeatureRegistryLoader;

#[derive(Debug, Clone)]
pub struct FilesystemFeatureRegistryLoader {
    resolution: FeaturePackResolution,
}

impl FilesystemFeatureRegistryLoader {
    pub fn new(resolution: FeaturePackResolution) -> Self {
        Self { resolution }
    }

    fn load_file(path: &Path) -> Result<FeatureRegistry, FeatureRegistryError> {
        let raw = fs::read_to_string(path)
            .map_err(|e| FeatureRegistryError::Io(format!("{}: {e}", path.display())))?;
        FeatureRegistry::load_from_str(&raw, &path.display().to_string())
    }
}

impl FeatureRegistryLoader for FilesystemFeatureRegistryLoader {
    fn load(&self) -> Result<FeatureRegistry, FeatureRegistryError> {
        match self.resolution.mode {
            EffectiveFeatureMode::Empty => Ok(FeatureRegistry::empty()),
            EffectiveFeatureMode::BaselineCompat => FeatureRegistry::baseline(),
            EffectiveFeatureMode::ExplicitFiles => {
                let mut merged = FeatureRegistry::empty();
                for path in &self.resolution.config.feature_files {
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
    use std::fs;

    #[test]
    fn baseline_compat_mode_loads_baseline_pack() {
        let loader = FilesystemFeatureRegistryLoader::new(
            crate::domain::FeaturePackResolution::baseline_compat(),
        );
        let registry = loader.load().expect("load");
        assert!(!registry.feature_ids().is_empty());
        assert!(registry.feature_ids().contains(&"inspect_error"));
    }

    #[test]
    fn empty_mode_yields_empty_registry() {
        let loader =
            FilesystemFeatureRegistryLoader::new(crate::domain::FeaturePackResolution::empty());
        let registry = loader.load().expect("load");
        assert!(registry.feature_ids().is_empty());
    }

    #[test]
    fn explicit_files_load_listed_feature_files_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let feature_path = dir.path().join("custom-features.toml");
        fs::write(
            &feature_path,
            r#"
[custom_feature]
description = "custom"
triggers = ["hello"]
"#,
        )
        .expect("write feature file");
        let loader =
            FilesystemFeatureRegistryLoader::new(FeaturePackResolution::explicit_files(vec![
                feature_path,
            ]));
        let registry = loader.load().expect("load");
        assert_eq!(registry.feature_ids(), vec!["custom_feature"]);
    }
}
