//! 0043 Phase 2 — feature pack 境界の integration テスト。

use aibe::adapters::outbound::FilesystemFeatureRegistryLoader;
use aibe::ports::outbound::{FeatureRegistryLoader, MemoryConfig};

#[test]
fn generic_memory_config_yields_empty_feature_registry() {
    let loader = FilesystemFeatureRegistryLoader::new(MemoryConfig {
        enabled: true,
        kind_files: Some(vec![]),
        recipe_files: Some(vec![]),
        feature_files: None,
    });
    let registry = loader.load().expect("load");
    assert!(registry.feature_ids().is_empty());
}

#[test]
fn compat_mode_still_loads_baseline_features() {
    let loader = FilesystemFeatureRegistryLoader::new(MemoryConfig::default());
    let registry = loader.load().expect("load");
    assert!(registry.feature_ids().contains(&"inspect_error"));
}

#[test]
fn explicit_empty_feature_files_yields_empty_registry() {
    let loader = FilesystemFeatureRegistryLoader::new(MemoryConfig {
        enabled: true,
        kind_files: Some(vec![]),
        recipe_files: Some(vec![]),
        feature_files: Some(vec![]),
    });
    let registry = loader.load().expect("load");
    assert!(registry.feature_ids().is_empty());
}
