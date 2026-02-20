//! ドメイン型（Newtype）
//!
//! String / PathBuf を直接運ばず、意味のある型に包んで境界を明確にする。

pub mod dirs;
pub mod event;

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

pub use dirs::Dirs;

/// セッションディレクトリのパス
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDir(PathBuf);

impl SessionDir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

impl std::ops::Deref for SessionDir {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for SessionDir {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl From<PathBuf> for SessionDir {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

/// ホームディレクトリのパス
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeDir(PathBuf);

impl HomeDir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

impl std::ops::Deref for HomeDir {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for HomeDir {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl From<PathBuf> for HomeDir {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

/// Part ID（8文字 base62、辞書順＝時系列）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartId(String);

impl PartId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for PartId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for PartId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for PartId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// プロバイダ名（gemini, gpt, echo 等）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderName(String);

impl ProviderName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for ProviderName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for ProviderName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ProviderName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl AsRef<str> for ProviderName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// モデル名（gemini-2.0, gpt-4 等）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelName(String);

impl ModelName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for ModelName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for ModelName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ModelName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl AsRef<str> for ModelName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// --- Shell suggestion / pending input ---------------------------------------------------------

/// LLM から受け取る構造化シェルコマンド
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructuredCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

/// Shell への提案（構造化コマンド + 任意の表示用ヒント）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShellSuggestion {
    pub command: StructuredCommand,
    pub display_hint: Option<String>,
}

/// コマンドポリシー評価結果（allowlist / denylist 判定を抽象化）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyStatus {
    Allowed,
    NeedsWarning { reason: String },
    Blocked { reason: String },
}

/// 次回 PromptReady 時に注入する 1 行入力
///
/// - text: aish 側で生成・サニタイズ済みの 1 行のみ（改行・制御文字なし、タブは許可）
/// - policy: allowlist/deny に基づく評価結果
/// - created_at_unix_ms: 生成時刻（監査・デバッグ用）
/// - source: "tool:queue_shell_suggestion" 等の由来識別子
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingInput {
    pub text: String,
    pub policy: PolicyStatus,
    pub created_at_unix_ms: i64,
    pub source: String,
}
