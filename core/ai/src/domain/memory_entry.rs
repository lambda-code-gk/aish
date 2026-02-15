//! 永続メモリ 1 件のドメイン型
//!
//! 旧実装の metadata.json / entries/<id>.json と互換のフィールド。

use serde::{Deserialize, Serialize};

/// 永続メモリ 1 件（保存・検索・取得で共通）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub subject: String,
    pub timestamp: String,
    #[serde(default)]
    pub usage_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u64>,
}

impl MemoryEntry {
    pub fn new(
        id: impl Into<String>,
        content: impl Into<String>,
        category: impl Into<String>,
        keywords: Vec<String>,
        subject: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            content: content.into(),
            category: category.into(),
            keywords,
            subject: subject.into(),
            timestamp: timestamp.into(),
            usage_count: 0,
            memory_dir: None,
            project_root: None,
            source: None,
            score: None,
        }
    }

    /// メタデータのみ（content を除く）で検索結果として返す用
    #[allow(dead_code)]
    pub fn meta_only(&self) -> MemoryMeta {
        MemoryMeta {
            id: self.id.clone(),
            category: self.category.clone(),
            keywords: self.keywords.clone(),
            subject: self.subject.clone(),
            timestamp: self.timestamp.clone(),
            usage_count: self.usage_count,
            memory_dir: self.memory_dir.clone(),
            project_root: self.project_root.clone(),
            source: self.source.clone(),
            score: self.score,
        }
    }
}

/// 検索結果用のメタデータ（content なし）。metadata.json の memories 要素と互換。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMeta {
    pub id: String,
    pub category: String,
    pub keywords: Vec<String>,
    pub subject: String,
    pub timestamp: String,
    #[serde(default)]
    pub usage_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u64>,
}
