//! 0043 — feature pack 境界の integration テスト。

use aibe::adapters::outbound::FilesystemFeatureRegistryLoader;
use aibe::domain::EffectiveFeatureMode;
use aibe::ports::outbound::{FeatureRegistryLoader, MemoryConfig};

#[test]
fn generic_memory_config_yields_empty_feature_registry() {
    let memory = MemoryConfig {
        enabled: true,
        kind_files: Some(vec![]),
        recipe_files: Some(vec![]),
        feature_files: None,
    };
    let resolution = memory.resolve_feature_pack();
    assert_eq!(resolution.mode, EffectiveFeatureMode::Empty);
    assert!(resolution.config.feature_files.is_empty());
    let registry = FilesystemFeatureRegistryLoader::new(resolution)
        .load()
        .expect("load");
    assert!(registry.feature_ids().is_empty());
}

#[test]
fn compat_mode_still_loads_baseline_features() {
    let memory = MemoryConfig::default();
    let resolution = memory.resolve_feature_pack();
    assert_eq!(resolution.mode, EffectiveFeatureMode::BaselineCompat);
    assert!(resolution.config.feature_files.is_empty());
    let registry = FilesystemFeatureRegistryLoader::new(resolution)
        .load()
        .expect("load");
    assert!(registry.feature_ids().contains(&"inspect_error"));
}

#[test]
fn explicit_empty_feature_files_yields_empty_registry() {
    let memory = MemoryConfig {
        enabled: true,
        kind_files: Some(vec![]),
        recipe_files: Some(vec![]),
        feature_files: Some(vec![]),
    };
    let resolution = memory.resolve_feature_pack();
    assert_eq!(resolution.mode, EffectiveFeatureMode::Empty);
    assert!(resolution.config.feature_files.is_empty());
    let registry = FilesystemFeatureRegistryLoader::new(resolution)
        .load()
        .expect("load");
    assert!(registry.feature_ids().is_empty());
}

#[test]
fn resolve_feature_pack_does_not_couple_kind_recipe_to_loader() {
    let memory = MemoryConfig {
        enabled: true,
        kind_files: Some(vec![]),
        recipe_files: Some(vec![]),
        feature_files: None,
    };
    let resolution = memory.resolve_feature_pack();
    assert_eq!(resolution.mode, EffectiveFeatureMode::Empty);
    let loader_only_pack = FilesystemFeatureRegistryLoader::new(resolution);
    assert!(loader_only_pack
        .load()
        .expect("load")
        .feature_ids()
        .is_empty());
}
