//! 永続メモリのドメイン型（list / get 表示用）
//!
//! ai の metadata.json / entries/<id>.json と同一形式でデシリアライズする。

use serde::Deserialize;

/// 一覧用（content なし）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used by main for display and by adapter for deserialize
pub struct MemoryListEntry {
    pub id: String,
    pub category: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub subject: String,
    pub timestamp: String,
}

/// 1 件取得用（content あり）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields used by main for display and by adapter for deserialize
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub category: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub subject: String,
    pub timestamp: String,
}
