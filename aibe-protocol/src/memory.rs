//! contextual memory wire DTO。

use serde::{Deserialize, Serialize};

/// memory RPC 用コンテキスト。`project_key` はサーバ導出。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
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
    pub memory_space_id: String,
    pub created_session_id: String,
    pub last_session_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryOperationAdd {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MemoryScopeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject: Option<MemoryInjectPolicyDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatusDto>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub make_active: Option<bool>,
}

/// `ClientRequest::MemoryKindList` の payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryKindListRequestBody {
    pub id: String,
    pub session_id: String,
    pub context: MemoryContext,
}

/// `ClientRequest::MemoryRecipeRun` の payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecipeRunRequestBody {
    pub id: String,
    pub session_id: String,
    pub context: MemoryContext,
    pub recipe: String,
    #[serde(default)]
    pub apply: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_instruction: Option<String>,
}

/// recipe 提案 1 件（operation + 表示専用 rationale）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecipeProposalDto {
    pub operation: MemoryOperationDto,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecipeStatus {
    Proposed,
    Applied,
}

/// registry kind 定義の wire DTO。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryKindDefinitionDto {
    pub id: String,
    pub description: String,
    pub default_scope: MemoryScopeDto,
    pub default_inject: MemoryInjectPolicyDto,
    pub default_status: MemoryStatusDto,
    pub lifecycle: String,
    pub cardinality: String,
    pub clear_from: MemoryStatusDto,
    pub clear_to: MemoryStatusDto,
    pub auto_inject: bool,
    pub on_demand: bool,
    pub priority: u32,
    pub keywords: Vec<String>,
    pub max_entries: Option<u32>,
    pub aliases: Vec<String>,
    pub builtin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedicated_cli: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryOperationClearKind {
    pub kind: String,
    pub scope: MemoryScopeDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryOperationArchive {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MemoryOperationDto {
    Add(MemoryOperationAdd),
    ClearKind(MemoryOperationClearKind),
    Archive(MemoryOperationArchive),
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

/// `ClientRequest::MemorySubscribe` の payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySubscribeRequestBody {
    pub id: String,
    pub session_id: String,
    pub context: MemoryContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySubscribeStatus {
    Ok,
}

/// memory 変更通知の種別。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryChangeKind {
    Added,
    StatusChanged,
    Archived,
    RecipeApplied,
}

/// `ClientResponse::MemoryChanged.event`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryChangeEventDto {
    pub kind: String,
    pub change: MemoryChangeKind,
    pub entries: Vec<MemoryEntryDto>,
}

/// `MemoryEntry.text` の最大長（バイト）。
pub const MEMORY_TEXT_MAX_BYTES: usize = 8 * 1024;

/// prompt 注入バジェット（バイト）。
pub const MEMORY_PROMPT_BUDGET_BYTES: usize = 4 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_operation_rejects_unknown_fields() {
        let json = r#"{"op":"add","kind":"goal","scope":"project","inject":"pinned","status":"active","text":"x","unknown":true}"#;
        assert!(serde_json::from_str::<MemoryOperationDto>(json).is_err());
    }

    #[test]
    fn memory_operation_add_accepts_omitted_fields_for_registered_kind() {
        let json = r#"{"op":"add","kind":"rule","text":"no shell auto-exec"}"#;
        let op: MemoryOperationDto = serde_json::from_str(json).expect("deserialize");
        match op {
            MemoryOperationDto::Add(add) => {
                assert_eq!(add.kind, "rule");
                assert_eq!(add.text, "no shell auto-exec");
                assert!(add.scope.is_none());
                assert!(add.inject.is_none());
                assert!(add.status.is_none());
                assert!(add.make_active.is_none());
            }
            _ => panic!("expected add"),
        }
    }

    #[test]
    fn memory_kind_definition_dto_roundtrip() {
        let dto = MemoryKindDefinitionDto {
            id: "goal".into(),
            description: "作業の最終目的".into(),
            default_scope: MemoryScopeDto::Project,
            default_inject: MemoryInjectPolicyDto::Pinned,
            default_status: MemoryStatusDto::Active,
            lifecycle: "active_inactive".into(),
            cardinality: "single_effective".into(),
            clear_from: MemoryStatusDto::Active,
            clear_to: MemoryStatusDto::Inactive,
            auto_inject: true,
            on_demand: false,
            priority: 10,
            keywords: vec![],
            max_entries: Some(1),
            aliases: vec!["goal".into()],
            builtin: true,
            dedicated_cli: Some("ai goal set".into()),
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        let back: MemoryKindDefinitionDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, "goal");
        assert_eq!(back.priority, 10);
    }

    #[test]
    fn memory_kind_list_roundtrip() {
        let req = serde_json::json!({
            "type": "memory_kind_list",
            "id": "k1",
            "session_id": "sess-1",
            "context": { "cwd": "/tmp/proj", "memory_space_id": "ctx_a" }
        });
        let json = serde_json::to_string(&req).expect("serialize");
        let back: serde_json::Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back["type"], "memory_kind_list");
        assert_eq!(back["session_id"], "sess-1");
    }

    #[test]
    fn memory_apply_roundtrip() {
        let req = serde_json::json!({
            "type": "memory_apply",
            "id": "m1",
            "session_id": "sess-1",
            "context": { "cwd": "/tmp/proj", "memory_space_id": "ctx_a" },
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
        let op = MemoryOperationDto::ClearKind(MemoryOperationClearKind {
            kind: "goal".into(),
            scope: MemoryScopeDto::Project,
        });
        let json = serde_json::to_string(&op).expect("serialize");
        let back: MemoryOperationDto = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            back,
            MemoryOperationDto::ClearKind(c) if c.kind == "goal"
        ));
    }

    #[test]
    fn memory_entry_dto_roundtrip() {
        let entry = MemoryEntryDto {
            id: "mem_01".into(),
            memory_space_id: "ctx_a".into(),
            created_session_id: "s-1".into(),
            last_session_id: "s-1".into(),
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

    #[test]
    fn memory_recipe_run_roundtrip() {
        let req = serde_json::json!({
            "type": "memory_recipe_run",
            "id": "r1",
            "session_id": "sess-1",
            "context": { "cwd": "/tmp/proj", "memory_space_id": "ctx_a" },
            "recipe": "clarify-goal",
            "apply": false,
            "user_instruction": "focus on MVP"
        });
        let json = serde_json::to_string(&req).expect("serialize");
        let back: serde_json::Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back["type"], "memory_recipe_run");
        assert_eq!(back["recipe"], "clarify-goal");
    }

    #[test]
    fn memory_recipe_proposal_dto_roundtrip() {
        let proposal = MemoryRecipeProposalDto {
            operation: MemoryOperationDto::Add(MemoryOperationAdd {
                kind: "goal".into(),
                scope: None,
                inject: None,
                status: None,
                text: "ship memory v1".into(),
                make_active: None,
            }),
            rationale: "consolidates open ideas".into(),
        };
        let json = serde_json::to_string(&proposal).expect("serialize");
        let back: MemoryRecipeProposalDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.rationale, "consolidates open ideas");
        match back.operation {
            MemoryOperationDto::Add(add) => assert_eq!(add.kind, "goal"),
            _ => panic!("expected add"),
        }
    }

    #[test]
    fn memory_recipe_proposal_rejects_unknown_fields() {
        let json =
            r#"{"operation":{"op":"add","kind":"goal","text":"x"},"rationale":"y","extra":1}"#;
        assert!(serde_json::from_str::<MemoryRecipeProposalDto>(json).is_err());
    }

    #[test]
    fn memory_subscribe_request_roundtrip() {
        let req = serde_json::json!({
            "type": "memory_subscribe",
            "id": "s1",
            "session_id": "sess-1",
            "context": { "cwd": "/tmp/proj", "memory_space_id": "ctx_a" },
            "kind": "goal"
        });
        let json = serde_json::to_string(&req).expect("serialize");
        let back: serde_json::Value = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back["type"], "memory_subscribe");
        assert_eq!(back["kind"], "goal");
    }

    #[test]
    fn memory_change_event_dto_roundtrip() {
        let event = MemoryChangeEventDto {
            kind: "goal".into(),
            change: MemoryChangeKind::Added,
            entries: vec![MemoryEntryDto {
                id: "mem_01".into(),
                memory_space_id: "ctx_a".into(),
                created_session_id: "s-1".into(),
                last_session_id: "s-1".into(),
                kind: "goal".into(),
                scope: MemoryScopeDto::Project,
                inject: MemoryInjectPolicyDto::Pinned,
                status: MemoryStatusDto::Active,
                text: "ship".into(),
                project_key: None,
                created_at_ms: 1,
                updated_at_ms: 1,
                version: 1,
            }],
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let back: MemoryChangeEventDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.kind, "goal");
        assert_eq!(back.change, MemoryChangeKind::Added);
    }
}
