//! `write_file` / `apply_patch` 共通の prepare 補助（設計 §24.4）。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::outbound::path_mode;
use crate::domain::{
    check_file_size, prepare_file_change_plan, reject_mixed_line_endings,
    sanitize_write_file_arguments, sha256_hex, validate_utf8_bytes, FileChangeOperation,
    FileSnapshot, FileTextError,
};
use crate::ports::outbound::ToolExecutionContext;

use super::safe_path::{SafePathError, WritePathPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFileMode {
    Create,
    Replace,
}

impl WriteFileMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Create),
            "replace" => Some(Self::Replace),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
        }
    }
}

pub(crate) fn map_path_error(err: SafePathError) -> (&'static str, String) {
    (err.code, err.message)
}

pub(crate) fn map_text_error(err: FileTextError) -> (&'static str, String) {
    (err.code(), err.code().to_string())
}

pub(crate) fn validate_write_content(
    content: &str,
    max_bytes: usize,
) -> Result<(), (&'static str, String)> {
    let bytes = content.as_bytes();
    if let Err(err) = check_file_size(bytes.len(), max_bytes) {
        return Err(map_text_error(err));
    }
    validate_utf8_bytes(bytes).map_err(map_text_error)?;
    reject_mixed_line_endings(content).map_err(map_text_error)?;
    Ok(())
}

pub(crate) async fn resolve_write_target(
    path_policy: &WritePathPolicy,
    ctx: &ToolExecutionContext,
    path_str: &str,
) -> Result<PathBuf, (&'static str, String)> {
    let rel = WritePathPolicy::validate_path_string(path_str).map_err(map_path_error)?;
    path_policy
        .resolve_write_path(ctx, &rel)
        .await
        .map_err(map_path_error)
}

pub(crate) async fn load_before_snapshot(
    canonical: &Path,
    mode: WriteFileMode,
) -> Result<FileSnapshot, (&'static str, String)> {
    match mode {
        WriteFileMode::Create => {
            if canonical.exists() {
                return Err(("target_exists", "target file already exists".into()));
            }
            let parent = canonical
                .parent()
                .ok_or(("parent_not_found", "parent directory does not exist".into()))?;
            if !parent.is_dir() {
                return Err(("parent_not_found", "parent directory does not exist".into()));
            }
            Ok(FileSnapshot::absent())
        }
        WriteFileMode::Replace => {
            if !canonical.is_file() {
                return Err(("target_not_found", "target file does not exist".into()));
            }
            let bytes = tokio::fs::read(canonical)
                .await
                .map_err(|e| ("target_not_found", e.to_string()))?;
            if let Err(err) = check_file_size(bytes.len(), usize::MAX) {
                return Err(map_text_error(err));
            }
            let content = validate_utf8_bytes(&bytes).map_err(map_text_error)?;
            reject_mixed_line_endings(&content).map_err(map_text_error)?;
            let hash = sha256_hex(&bytes);
            let file_mode = path_mode(canonical).ok();
            Ok(FileSnapshot::present(bytes, hash, file_mode))
        }
    }
}

pub(crate) fn verify_expected_hash(
    mode: WriteFileMode,
    expected_sha256: Option<&str>,
    before: &FileSnapshot,
) -> Result<(), (&'static str, String)> {
    match mode {
        WriteFileMode::Create => {
            if expected_sha256.is_some() {
                return Err((
                    "invalid_arguments",
                    "expected_sha256 must not be set for mode=create".into(),
                ));
            }
            Ok(())
        }
        WriteFileMode::Replace => {
            let Some(expected) = expected_sha256 else {
                return Err((
                    "precondition_required",
                    "expected_sha256 is required for mode=replace".into(),
                ));
            };
            let Some(actual) = before.sha256.as_deref() else {
                return Err(("stale_file", "file hash mismatch".into()));
            };
            if expected != actual {
                return Err(("stale_file", "file hash mismatch".into()));
            }
            Ok(())
        }
    }
}

pub(crate) fn build_write_file_plan(
    canonical: PathBuf,
    mode: WriteFileMode,
    before: FileSnapshot,
    content: &str,
    max_preview_bytes: usize,
) -> crate::domain::FileChangePlan {
    let operation = match mode {
        WriteFileMode::Create => FileChangeOperation::Create,
        WriteFileMode::Replace => FileChangeOperation::Replace,
    };
    let display = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| canonical.display().to_string());
    prepare_file_change_plan(
        canonical,
        operation,
        before,
        content.as_bytes().to_vec(),
        &display,
        max_preview_bytes,
    )
}

pub(crate) fn sanitized_write_file_args(
    path: &str,
    mode: WriteFileMode,
    expected_sha256: Option<&str>,
    content: &str,
) -> Value {
    sanitize_write_file_arguments(path, mode.as_str(), expected_sha256, content)
}
