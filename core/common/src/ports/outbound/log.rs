//! 構造化ログ Outbound ポート
//!
//! 全レイヤー（CLI / usecase / adapter）から JSONL ログをファイルに出力するための trait。
//! エラー時のコンソール表示（stderr）とは別チャネルで、ファイルにのみ書き出す。

use crate::error::Error;
use serde::Serialize;
use std::collections::BTreeMap;

/// 現在時刻を ISO8601 (RFC3339) で返す。LogRecord の `ts` に使う。
pub fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// ログレベル
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// 1 行分のログレコード（JSONL の 1 行に対応）
#[derive(Debug, Clone, Serialize)]
pub struct LogRecord {
    /// ISO8601 形式のタイムスタンプ
    pub ts: String,
    pub level: LogLevel,
    pub message: String,
    /// 例: cli, usecase, adapter, wiring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// 例: lifecycle, cli, config, usecase, adapter, error, perf
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 追加のキー・値（オブジェクトとして出力）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, serde_json::Value>>,
}

/// 構造化ログを出力する Outbound ポート
///
/// 実装は common::adapter::FileJsonLog（ファイルへ JSONL 追記）や NoopLog（テスト用）など。
pub trait Log: Send + Sync {
    /// 1 レコードをログに書き出す（ファイルへ JSONL 1 行として追記）
    fn log(&self, record: &LogRecord) -> Result<(), Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_record_serialize() {
        let rec = LogRecord {
            ts: "2026-02-07T12:00:00Z".to_string(),
            level: LogLevel::Info,
            message: "command started".to_string(),
            layer: Some("cli".to_string()),
            kind: Some("lifecycle".to_string()),
            fields: {
                let mut m = BTreeMap::new();
                m.insert("command".to_string(), serde_json::json!("query"));
                Some(m)
            },
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"ts\":\"2026-02-07T12:00:00Z\""));
        assert!(json.contains("\"level\":\"info\""));
        assert!(json.contains("\"message\":\"command started\""));
        assert!(json.contains("\"layer\":\"cli\""));
        assert!(json.contains("\"kind\":\"lifecycle\""));
        assert!(json.contains("\"command\""));
    }
}
