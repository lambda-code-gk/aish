//! ファイル変更計画のドメイン型（設計 §16–§17, §24.2）。

use std::path::PathBuf;

use super::sha256_hex;

/// 変更種別（設計 §16.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeOperation {
    Create,
    Replace,
    Patch,
}

impl FileChangeOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
            Self::Patch => "patch",
        }
    }
}

/// 変更前のファイル状態（設計 §19.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeforeState {
    Absent,
    Present,
}

/// 変更前スナップショット（設計 §24.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    pub before_state: BeforeState,
    pub bytes: Option<Vec<u8>>,
    pub sha256: Option<String>,
    pub file_mode: Option<u32>,
}

impl FileSnapshot {
    pub fn absent() -> Self {
        Self {
            before_state: BeforeState::Absent,
            bytes: None,
            sha256: None,
            file_mode: None,
        }
    }

    pub fn present(bytes: Vec<u8>, sha256: String, file_mode: Option<u32>) -> Self {
        Self {
            before_state: BeforeState::Present,
            bytes: Some(bytes),
            sha256: Some(sha256),
            file_mode,
        }
    }
}

/// 差分サマリ（設計 §16.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSummary {
    pub operation: FileChangeOperation,
    pub lines_added: usize,
    pub lines_removed: usize,
    pub before_bytes: usize,
    pub after_bytes: usize,
}

impl DiffSummary {
    /// 人間向け 1 行サマリ（設計 §16.2 例）。
    pub fn display_line(&self, display_path: &str) -> String {
        let verb = match self.operation {
            FileChangeOperation::Create => "create",
            FileChangeOperation::Replace | FileChangeOperation::Patch => "modify",
        };
        format!(
            "{verb} {display_path} (+{} -{}, {} -> {} bytes)",
            self.lines_added, self.lines_removed, self.before_bytes, self.after_bytes
        )
    }
}

/// 承認 UI 向け diff preview（設計 §16）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPreview {
    pub diff_text: String,
    pub summary: DiffSummary,
    pub preview_truncated: bool,
}

/// 承認前に確定した変更候補（設計 §24.2 — Phase 5 で使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChangePlan {
    pub path: PathBuf,
    pub operation: FileChangeOperation,
    pub before: FileSnapshot,
    pub after_bytes: Vec<u8>,
    pub before_sha256: Option<String>,
    pub after_sha256: String,
    pub diff_preview: DiffPreview,
}

/// `write_file` 監査用 sanitized arguments（設計 §20.1）。
pub fn sanitize_write_file_arguments(
    path: &str,
    mode: &str,
    expected_sha256: Option<&str>,
    content: &str,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "path": path,
        "mode": mode,
        "content_bytes": content.len(),
    });
    if let Some(hash) = expected_sha256 {
        value["expected_sha256"] = serde_json::Value::String(hash.to_string());
    }
    value
}

/// prepare 段階の plan 組み立て（ファイルは変更しない）。
pub fn prepare_file_change_plan(
    path: PathBuf,
    operation: FileChangeOperation,
    before: FileSnapshot,
    after_bytes: Vec<u8>,
    display_path: &str,
    max_preview_bytes: usize,
) -> FileChangePlan {
    let before_sha256 = before.sha256.clone();
    let after_sha256 = sha256_hex(&after_bytes);
    let before_bytes = before.bytes.as_deref();
    let diff_preview = crate::domain::build_unified_diff_preview(
        display_path,
        before_bytes,
        &after_bytes,
        operation,
        max_preview_bytes,
    );
    FileChangePlan {
        path,
        operation,
        before,
        after_bytes,
        before_sha256,
        after_sha256,
        diff_preview,
    }
}

/// `apply_patch` 監査用 sanitized arguments（設計 §20.1）。
pub fn sanitize_apply_patch_arguments(
    path: &str,
    expected_sha256: &str,
    patch: &str,
    hunk_count: usize,
) -> serde_json::Value {
    serde_json::json!({
        "path": path,
        "expected_sha256": expected_sha256,
        "patch_bytes": patch.len(),
        "hunk_count": hunk_count,
    })
}

/// 監査記録用にツール引数から機密フィールドを除去する（best-effort）。
pub fn sanitize_tool_arguments_for_audit(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> serde_json::Value {
    match tool_name {
        crate::domain::WRITE_FILE => sanitize_write_file_arguments_best_effort(arguments),
        crate::domain::APPLY_PATCH => sanitize_apply_patch_arguments_best_effort(arguments),
        _ => sanitize_arguments_fallback(arguments, &[]),
    }
}

/// `write_file` 引数の best-effort サニタイズ（parse 失敗時も content を残さない）。
pub fn sanitize_write_file_arguments_best_effort(
    arguments: &serde_json::Value,
) -> serde_json::Value {
    #[derive(serde::Deserialize)]
    struct Args {
        path: Option<String>,
        mode: Option<String>,
        content: Option<String>,
        expected_sha256: Option<String>,
    }

    if let Ok(parsed) = serde_json::from_value::<Args>(arguments.clone()) {
        let path = parsed.path.unwrap_or_default();
        let mode = parsed.mode.unwrap_or_else(|| "unknown".to_string());
        let content_len = parsed.content.map(|c| c.len()).unwrap_or(0);
        let mut value = serde_json::json!({
            "path": path,
            "mode": mode,
            "content_bytes": content_len,
        });
        if let Some(hash) = parsed.expected_sha256 {
            value["expected_sha256"] = serde_json::Value::String(hash);
        }
        return value;
    }
    sanitize_arguments_fallback(arguments, &["content"])
}

/// `apply_patch` 引数の best-effort サニタイズ（parse 失敗時も patch を残さない）。
pub fn sanitize_apply_patch_arguments_best_effort(
    arguments: &serde_json::Value,
) -> serde_json::Value {
    #[derive(serde::Deserialize)]
    struct Args {
        path: Option<String>,
        patch: Option<String>,
        expected_sha256: Option<String>,
    }

    if let Ok(parsed) = serde_json::from_value::<Args>(arguments.clone()) {
        let path = parsed.path.unwrap_or_default();
        let patch_len = parsed.patch.map(|p| p.len()).unwrap_or(0);
        let mut value = serde_json::json!({
            "path": path,
            "patch_bytes": patch_len,
            "hunk_count": 0,
        });
        if let Some(hash) = parsed.expected_sha256 {
            value["expected_sha256"] = serde_json::Value::String(hash);
        }
        return value;
    }
    sanitize_arguments_fallback(arguments, &["patch"])
}

/// 任意 JSON 引数から機密キーをバイト数に置換する。
pub fn sanitize_arguments_fallback(
    arguments: &serde_json::Value,
    strip_keys: &[&str],
) -> serde_json::Value {
    match arguments {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            let mut keys = Vec::new();
            for (key, value) in map {
                keys.push(key.clone());
                if strip_keys.contains(&key.as_str()) {
                    let byte_len = match value {
                        serde_json::Value::String(s) => s.len(),
                        other => other.to_string().len(),
                    };
                    out.insert(
                        format!("{key}_bytes"),
                        serde_json::Value::Number(byte_len.into()),
                    );
                } else {
                    out.insert(key.clone(), value.clone());
                }
            }
            keys.sort();
            out.insert(
                "argument_keys".to_string(),
                serde_json::Value::Array(keys.into_iter().map(serde_json::Value::String).collect()),
            );
            serde_json::Value::Object(out)
        }
        other => serde_json::json!({
            "argument_bytes": other.to_string().len(),
        }),
    }
}
