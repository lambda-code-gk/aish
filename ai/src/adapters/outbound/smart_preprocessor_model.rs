//! Smart Preprocessor model artifact の読み込みと検証。

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::domain::smart_preprocessor::{DEFAULT_MODEL_VERSION, FEATURE_EXTRACTOR_VERSION};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPreprocessorModel {
    pub model_version: String,
    pub feature_extractor_version: String,
}

#[derive(Debug, Deserialize)]
struct ModelFile {
    model_version: String,
    feature_extractor_version: String,
    #[serde(default)]
    _dimension: Option<u32>,
    #[allow(dead_code)]
    checksum: Option<String>,
}

pub fn load_preprocessor_model(path: &Path) -> Result<ValidatedPreprocessorModel, String> {
    if !path.is_file() {
        return Err(format!("model file not found: {}", path.display()));
    }
    let raw =
        fs::read_to_string(path).map_err(|e| format!("read model {}: {e}", path.display()))?;
    let file: ModelFile =
        serde_json::from_str(&raw).map_err(|e| format!("parse model {}: {e}", path.display()))?;
    if file.model_version.trim().is_empty() {
        return Err("model_version must not be empty".into());
    }
    if file.feature_extractor_version != FEATURE_EXTRACTOR_VERSION {
        return Err(format!(
            "feature_extractor_version mismatch: expected {FEATURE_EXTRACTOR_VERSION}, got {}",
            file.feature_extractor_version
        ));
    }
    if file.model_version != DEFAULT_MODEL_VERSION {
        return Err(format!(
            "unsupported model_version: expected {DEFAULT_MODEL_VERSION}, got {}",
            file.model_version
        ));
    }
    if let Some(ref checksum) = file.checksum {
        if checksum.trim().is_empty() {
            return Err("checksum must not be empty when present".into());
        }
    }
    Ok(ValidatedPreprocessorModel {
        model_version: file.model_version,
        feature_extractor_version: file.feature_extractor_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_rejects_missing_file() {
        let err = load_preprocessor_model(Path::new("/tmp/nonexistent-smart-model.json"))
            .expect_err("missing");
        assert!(err.contains("not found"));
    }

    #[test]
    fn load_rejects_feature_version_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.json");
        fs::write(
            &path,
            r#"{"model_version":"smart-lr-v1","feature_extractor_version":"wrong"}"#,
        )
        .expect("write");
        let err = load_preprocessor_model(&path).expect_err("mismatch");
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn load_rejects_unsupported_model_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.json");
        fs::write(
            &path,
            r#"{"model_version":"smart-lr-v0","feature_extractor_version":"smart-features-v1"}"#,
        )
        .expect("write");
        let err = load_preprocessor_model(&path).expect_err("unsupported");
        assert!(err.contains("unsupported model_version"));
    }

    #[test]
    fn load_accepts_valid_model() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.json");
        fs::write(
            &path,
            r#"{"model_version":"smart-lr-v1","feature_extractor_version":"smart-features-v1"}"#,
        )
        .expect("write");
        let model = load_preprocessor_model(&path).expect("load");
        assert_eq!(model.model_version, "smart-lr-v1");
    }
}
