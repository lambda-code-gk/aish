//! イベントドメイン（Transcript / HumanLog 用）
//!
//! 発火側が作る `Event` と、永続化用に ts/seq を埋めた `EventRecord` を定義する。

use serde::{Deserialize, Serialize};

/// セッション識別子
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for SessionId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// 実行識別子（1回の run 単位）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub String);

impl RunId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for RunId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// 発火側が作るイベント（ts/seq は未設定）
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// スキーマバージョン（将来拡張用）
    pub v: u32,
    /// セッションID
    pub session_id: SessionId,
    /// 実行ID
    pub run_id: RunId,
    /// 種別（例: "run.started", "run.completed", "run.failed"）
    pub kind: String,
    /// 任意ペイロード
    pub payload: serde_json::Value,
}

/// 永続化用イベント（ts/seq が埋まった最終形）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub v: u32,
    /// RFC3339 UTC
    pub ts: String,
    /// ファイル内連番
    pub seq: u64,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_newtype_construct() {
        let sid = SessionId::new("s1");
        let rid = RunId::new("r1");
        assert_eq!(sid.0, "s1");
        assert_eq!(rid.0, "r1");
    }
}
