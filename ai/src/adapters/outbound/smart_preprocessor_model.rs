//! Smart Preprocessor model artifact の読み込みと検証。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::smart_preprocessor::{
    PreprocessorModel, SmartIntentClass, SparseLogisticHead, DEFAULT_MODEL_VERSION,
    FEATURE_EXTRACTOR_VERSION,
};

/// リポジトリ外に配置した `ai` バイナリでも読めるよう、compile 時に埋め込む。
const BUNDLED_MODEL_JSON: &str = include_str!("../../../resources/smart_preprocessor_model.json");

#[derive(Debug, Clone)]
pub struct ValidatedPreprocessorModel {
    pub model: PreprocessorModel,
}

#[derive(Debug, Deserialize)]
struct ModelFile {
    model_version: String,
    feature_extractor_version: String,
    #[serde(default)]
    _dimension: Option<u32>,
    heads: ModelHeads,
    #[allow(dead_code)]
    checksum: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelHeads {
    intent: HashMap<String, HeadToml>,
    safety: HeadToml,
    gate: HeadToml,
}

#[derive(Debug, Deserialize)]
struct HeadToml {
    bias: f32,
    #[serde(default)]
    features: HashMap<String, f32>,
}

pub fn bundled_model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/smart_preprocessor_model.json")
}

pub fn load_bundled_preprocessor_model(
    feature_hash_buckets: u32,
    feature_hash_seed: u64,
) -> Result<ValidatedPreprocessorModel, String> {
    parse_preprocessor_model_json(
        BUNDLED_MODEL_JSON,
        "bundled",
        feature_hash_buckets,
        feature_hash_seed,
    )
}

pub fn load_preprocessor_model(
    path: &Path,
    feature_hash_buckets: u32,
    feature_hash_seed: u64,
) -> Result<ValidatedPreprocessorModel, String> {
    if !path.is_file() {
        return Err("model file not found".into());
    }
    let raw = fs::read_to_string(path).map_err(|_| "read model failed".to_string())?;
    parse_preprocessor_model_json(&raw, "file", feature_hash_buckets, feature_hash_seed)
}

fn parse_preprocessor_model_json(
    raw: &str,
    source_label: &str,
    feature_hash_buckets: u32,
    feature_hash_seed: u64,
) -> Result<ValidatedPreprocessorModel, String> {
    let file: ModelFile =
        serde_json::from_str(raw).map_err(|_| format!("parse model {source_label} failed"))?;
    if file.model_version.trim().is_empty() {
        return Err("model_version must not be empty".into());
    }
    if file.feature_extractor_version != FEATURE_EXTRACTOR_VERSION {
        return Err("feature_extractor_version mismatch".into());
    }
    if file.model_version != DEFAULT_MODEL_VERSION {
        return Err("unsupported model_version".into());
    }
    if file.heads.intent.is_empty() {
        return Err("intent heads must not be empty".into());
    }
    let intent_heads = file
        .heads
        .intent
        .into_iter()
        .map(|(name, head)| {
            let intent = parse_intent_head_name(&name)?;
            let sparse = resolve_head(head, feature_hash_buckets, feature_hash_seed)?;
            Ok((intent, sparse))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let safety_head = resolve_head(file.heads.safety, feature_hash_buckets, feature_hash_seed)?;
    let gate_head = resolve_head(file.heads.gate, feature_hash_buckets, feature_hash_seed)?;
    Ok(ValidatedPreprocessorModel {
        model: PreprocessorModel {
            model_version: file.model_version,
            feature_extractor_version: file.feature_extractor_version,
            intent_heads,
            safety_head,
            gate_head,
        },
    })
}

fn parse_intent_head_name(name: &str) -> Result<SmartIntentClass, String> {
    match name.trim() {
        "simple_chat" => Ok(SmartIntentClass::SimpleChat),
        "inspect" => Ok(SmartIntentClass::Inspect),
        "debug" => Ok(SmartIntentClass::Debug),
        "memory_lookup" => Ok(SmartIntentClass::MemoryLookup),
        "memory_recipe_hint" => Ok(SmartIntentClass::MemoryRecipeHint),
        "shell_command_candidate" => Ok(SmartIntentClass::ShellCommandCandidate),
        "retry" => Ok(SmartIntentClass::Retry),
        "rerun" => Ok(SmartIntentClass::Rerun),
        "ambiguous" => Ok(SmartIntentClass::Ambiguous),
        "unknown" => Ok(SmartIntentClass::Unknown),
        other => Err(format!("unsupported intent head: {other}")),
    }
}

fn resolve_head(head: HeadToml, buckets: u32, seed: u64) -> Result<SparseLogisticHead, String> {
    let mut weights = Vec::new();
    for (name, weight) in head.features {
        if !weight.is_finite() {
            return Err(format!("non-finite weight for feature {name}"));
        }
        let index = crate::domain::smart_preprocessor::hash_feature(&name, buckets, seed);
        weights.push((index, weight));
    }
    weights.sort_by_key(|(index, _)| *index);
    Ok(SparseLogisticHead {
        bias: head.bias,
        weights,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_rejects_missing_file() {
        let err =
            load_preprocessor_model(Path::new("/tmp/nonexistent-smart-model.json"), 262144, 17)
                .expect_err("missing");
        assert_eq!(err, "model file not found");
        assert!(!err.contains('/'));
    }

    #[test]
    fn load_rejects_feature_version_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.json");
        fs::write(
            &path,
            r#"{"model_version":"smart-lr-v1","feature_extractor_version":"wrong","heads":{"intent":{"simple_chat":{"bias":0.0,"features":{}}},"safety":{"bias":0.0,"features":{}},"gate":{"bias":0.0,"features":{}}}}"#,
        )
        .expect("write");
        let err = load_preprocessor_model(&path, 262144, 17).expect_err("mismatch");
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn load_accepts_bundled_model() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/smart_preprocessor_model.json");
        let model = load_preprocessor_model(&path, 262144, 17).expect("load");
        assert_eq!(model.model.model_version, "smart-lr-v1");
        assert!(model
            .model
            .intent_heads
            .contains_key(&SmartIntentClass::SimpleChat));
    }

    #[test]
    fn bundled_model_is_used_when_model_path_is_missing() {
        let model = load_bundled_preprocessor_model(262144, 17).expect("bundled");
        assert_eq!(model.model.model_version, DEFAULT_MODEL_VERSION);
        assert_eq!(
            model.model.feature_extractor_version,
            FEATURE_EXTRACTOR_VERSION
        );
    }

    #[test]
    fn bundled_model_loads_from_embedded_json_without_filesystem() {
        let model = parse_preprocessor_model_json(BUNDLED_MODEL_JSON, "bundled", 262144, 17)
            .expect("embedded");
        assert_eq!(model.model.model_version, DEFAULT_MODEL_VERSION);
    }
}
