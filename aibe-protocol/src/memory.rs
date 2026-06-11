//! contextual memory wire DTO。

use serde::{Deserialize, Serialize};

/// memory RPC 用コンテキスト（`cwd` のみ。`project_key` はサーバ導出）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryContext {
    pub cwd: String,
}

/// `ClientRequest::MemoryApply` の payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryApplyRequestBody {
    pub id: String,
    pub session_id: String,
    pub context: MemoryContext,
    pub operation: MemoryOperationDto,
}

/// `ClientRequest::MemoryQuery` の payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryQueryRequestBody {
    pub id: String,
    pub session_id: String,
    pub context: MemoryContext,
    pub query: MemoryQueryDto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScopeDto {
    Session,
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryInjectPolicyDto {
    Pinned,
    OnDemand,
    Manual,
    Never,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatusDto {
    Active,
    Inactive,
    Open,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryEntryDto {
    pub id: String,
    pub session_id: String,
    pub kind: String,
    pub scope: MemoryScopeDto,
    pub inject: MemoryInjectPolicyDto,
    pub status: MemoryStatusDto,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_key: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MemoryOperationDto {
    Add {
        kind: String,
        scope: MemoryScopeDto,
        inject: MemoryInjectPolicyDto,
        status: MemoryStatusDto,
        text: String,
        #[serde(default)]
        make_active: bool,
    },
    ClearActive {
        kind: String,
        scope: MemoryScopeDto,
    },
    Archive {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_version: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryQueryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MemoryScopeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatusDto>,
    #[serde(default)]
    pub active_only: bool,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// `true` のとき `resolve_for_prompt` 相当の materialized block を応答に含める。
    #[serde(default)]
    pub include_prompt_block: bool,
    /// on-demand idea 解決用のユーザー query（`include_prompt_block` 時のみ参照）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_query: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryApplyStatus {
    Ok,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryQueryStatus {
    Ok,
}

/// `MemoryEntry.text` の最大長（バイト）。
pub const MEMORY_TEXT_MAX_BYTES: usize = 8 * 1024;

/// prompt 注入バジェット（バイト）。
pub const MEMORY_PROMPT_BUDGET_BYTES: usize = 4 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_apply_roundtrip() {
        let req = serde_json::json!({
            "type": "memory_apply",
            "id": "m1",
            "session_id": "sess-1",
            "context": { "cwd": "/tmp/proj" },
            "operation": {
                "op": "add",
                "kind": "goal",
                "scope": "project",
                "inject": "pinned",
                "status": "active",
                "text": "ship it",
                "make_active": true
            }
        });
        let json = serde_json::to_string(&req).expect("serialize");
        let back: serde_json::Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back["type"], "memory_apply");
        assert_eq!(back["operation"]["op"], "add");
    }

    #[test]
    fn memory_operation_dto_roundtrip() {
        let op = MemoryOperationDto::ClearActive {
            kind: "goal".into(),
            scope: MemoryScopeDto::Project,
        };
        let json = serde_json::to_string(&op).expect("serialize");
        let back: MemoryOperationDto = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            back,
            MemoryOperationDto::ClearActive { kind, .. } if kind == "goal"
        ));
    }

    #[test]
    fn memory_entry_dto_roundtrip() {
        let entry = MemoryEntryDto {
            id: "mem_01".into(),
            session_id: "s-1".into(),
            kind: "goal".into(),
            scope: MemoryScopeDto::Project,
            inject: MemoryInjectPolicyDto::Pinned,
            status: MemoryStatusDto::Active,
            text: "do thing".into(),
            project_key: Some("/tmp/proj".into()),
            created_at_ms: 1,
            updated_at_ms: 2,
            version: 1,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: MemoryEntryDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, "mem_01");
        assert_eq!(back.kind, "goal");
    }

    #[test]
    fn memory_query_dto_roundtrip() {
        let q = MemoryQueryDto {
            kind: Some("idea".into()),
            scope: Some(MemoryScopeDto::Project),
            status: Some(MemoryStatusDto::Open),
            active_only: false,
            include_archived: false,
            limit: Some(20),
            include_prompt_block: false,
            user_query: None,
        };
        let json = serde_json::to_string(&q).expect("serialize");
        let back: MemoryQueryDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.kind.as_deref(), Some("idea"));
        assert_eq!(back.limit, Some(20));
    }

    #[test]
    fn memory_query_dto_rejects_unknown_fields() {
        let json = r#"{"kind":"goal","unknown":true}"#;
        let err = serde_json::from_str::<MemoryQueryDto>(json);
        assert!(err.is_err());
    }
}
