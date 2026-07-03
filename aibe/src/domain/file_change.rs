//! ファイル変更計画のドメイン型（設計 §16–§17, §24.2）。

use std::path::PathBuf;

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
