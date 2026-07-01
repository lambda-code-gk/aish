//! `ai work` 用 wire DTO。

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

use crate::MemoryContext;

pub const WORK_TEXT_MAX_BYTES: usize = 8 * 1024;
pub const WORK_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum WorkInputError {
    #[error("work text must not be empty")]
    EmptyText,
    #[error("work text must not contain NUL")]
    ContainsNul,
    #[error("work text exceeds {WORK_TEXT_MAX_BYTES} bytes")]
    TextTooLong,
    #[error("work id must be a positive integer")]
    InvalidWorkId,
}

pub fn validate_work_text(text: &str) -> Result<(), WorkInputError> {
    if text.trim().is_empty() {
        return Err(WorkInputError::EmptyText);
    }
    if text.contains('\0') {
        return Err(WorkInputError::ContainsNul);
    }
    if text.len() > WORK_TEXT_MAX_BYTES {
        return Err(WorkInputError::TextTooLong);
    }
    Ok(())
}

pub fn validate_work_id(work_id: u64) -> Result<(), WorkInputError> {
    if work_id == 0 {
        Err(WorkInputError::InvalidWorkId)
    } else {
        Ok(())
    }
}

pub fn validate_optional_work_text(text: &str) -> Result<(), WorkInputError> {
    validate_work_text(text)
}

fn deserialize_work_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    validate_work_text(&text).map_err(D::Error::custom)?;
    Ok(text)
}

fn deserialize_work_id<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let work_id = u64::deserialize(deserializer)?;
    validate_work_id(work_id).map_err(D::Error::custom)?;
    Ok(work_id)
}

fn deserialize_optional_work_id<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let work_id = Option::<u64>::deserialize(deserializer)?;
    work_id
        .map(validate_work_id)
        .transpose()
        .map_err(D::Error::custom)?;
    Ok(work_id)
}

fn deserialize_work_ids<'de, D>(deserializer: D) -> Result<Vec<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let work_ids = Vec::<u64>::deserialize(deserializer)?;
    for work_id in &work_ids {
        validate_work_id(*work_id).map_err(D::Error::custom)?;
    }
    Ok(work_ids)
}

fn deserialize_optional_work_text<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if let Some(text) = &value {
        validate_optional_work_text(text).map_err(D::Error::custom)?;
    }
    Ok(value)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkApplyRequestBody {
    pub id: String,
    pub session_id: String,
    pub context: MemoryContext,
    pub operation: WorkOperationDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkQueryRequestBody {
    pub id: String,
    pub session_id: String,
    pub context: MemoryContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkOperationDto {
    Start {
        #[serde(deserialize_with = "deserialize_work_text")]
        goal: String,
    },
    Focus {
        #[serde(deserialize_with = "deserialize_work_text")]
        text: String,
    },
    AddEntry {
        kind: WorkEntryKindDto,
        #[serde(deserialize_with = "deserialize_work_text")]
        text: String,
    },
    Defer {
        #[serde(deserialize_with = "deserialize_work_text")]
        text: String,
    },
    Switch {
        #[serde(deserialize_with = "deserialize_work_id")]
        work_id: u64,
    },
    Push {
        #[serde(deserialize_with = "deserialize_work_text")]
        goal: String,
    },
    Pop,
    Finish,
}

impl WorkOperationDto {
    pub fn validate(&self) -> Result<(), WorkInputError> {
        match self {
            Self::Start { goal } | Self::Push { goal } => validate_work_text(goal),
            Self::Focus { text } | Self::AddEntry { text, .. } | Self::Defer { text } => {
                validate_work_text(text)
            }
            Self::Switch { work_id } => validate_work_id(*work_id),
            Self::Pop | Self::Finish => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkEntryKindDto {
    Note,
    Idea,
    Decision,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatusDto {
    Active,
    Paused,
    Deferred,
    Done,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkItemDto {
    #[serde(deserialize_with = "deserialize_work_id")]
    pub id: u64,
    #[serde(deserialize_with = "deserialize_work_text")]
    pub title: String,
    #[serde(deserialize_with = "deserialize_work_text")]
    pub goal: String,
    pub status: WorkStatusDto,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_work_id"
    )]
    pub parent_id: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_work_text"
    )]
    pub focus: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_work_text"
    )]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkEntryDto {
    #[serde(deserialize_with = "deserialize_work_id")]
    pub id: u64,
    #[serde(deserialize_with = "deserialize_work_id")]
    pub work_id: u64,
    pub kind: WorkEntryKindDto,
    #[serde(deserialize_with = "deserialize_work_text")]
    pub text: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkSnapshotDto {
    pub revision: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_work_id"
    )]
    pub active_work_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_work_ids")]
    pub stack: Vec<u64>,
    #[serde(default)]
    pub works: Vec<WorkItemDto>,
    #[serde(default)]
    pub entries: Vec<WorkEntryDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkMutationKindDto {
    Start,
    Focus,
    AddEntry,
    Defer,
    Switch,
    Push,
    Pop,
    Finish,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkMutationOutcomeDto {
    pub kind: WorkMutationKindDto,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_work_id"
    )]
    pub work_id: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_work_id"
    )]
    pub previous_work_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkApplyResponseBody {
    pub id: String,
    pub snapshot: WorkSnapshotDto,
    pub outcome: WorkMutationOutcomeDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkQueryResponseBody {
    pub id: String,
    pub snapshot: WorkSnapshotDto,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientRequest, ClientResponse};

    fn context() -> MemoryContext {
        MemoryContext {
            cwd: Some("/tmp/project".into()),
            memory_space_id: Some("project_test".into()),
        }
    }

    #[test]
    fn work_protocol_dto_roundtrip_and_rejects_unknown_fields() {
        let operations = [
            WorkOperationDto::Start {
                goal: "ship phase zero".into(),
            },
            WorkOperationDto::Focus {
                text: "protocol".into(),
            },
            WorkOperationDto::AddEntry {
                kind: WorkEntryKindDto::Decision,
                text: "keep state server-side".into(),
            },
            WorkOperationDto::Defer {
                text: "later".into(),
            },
            WorkOperationDto::Switch { work_id: 2 },
            WorkOperationDto::Push {
                goal: "child".into(),
            },
            WorkOperationDto::Pop,
            WorkOperationDto::Finish,
        ];
        for operation in operations {
            let apply = ClientRequest::WorkApply(WorkApplyRequestBody {
                id: "work-1".into(),
                session_id: "session-1".into(),
                context: context(),
                operation: operation.clone(),
            });
            let encoded = serde_json::to_string(&apply).expect("serialize apply");
            let decoded: ClientRequest = serde_json::from_str(&encoded).expect("deserialize apply");
            let ClientRequest::WorkApply(decoded) = decoded else {
                panic!("expected work apply");
            };
            assert_eq!(decoded.operation, operation);
        }

        let query = ClientRequest::WorkQuery(WorkQueryRequestBody {
            id: "work-2".into(),
            session_id: "session-1".into(),
            context: context(),
        });
        let encoded = serde_json::to_string(&query).expect("serialize query");
        let decoded: ClientRequest = serde_json::from_str(&encoded).expect("deserialize query");
        let ClientRequest::WorkQuery(decoded) = decoded else {
            panic!("expected work query");
        };
        assert_eq!(decoded.context, context());

        let outcome = WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Start,
            work_id: Some(1),
            previous_work_id: None,
        };
        let apply_response = ClientResponse::WorkApplyResult(WorkApplyResponseBody {
            id: "work-1".into(),
            snapshot: WorkSnapshotDto::default(),
            outcome: outcome.clone(),
        });
        let encoded = serde_json::to_string(&apply_response).expect("serialize apply response");
        let decoded: ClientResponse = serde_json::from_str(&encoded).expect("deserialize response");
        let ClientResponse::WorkApplyResult(WorkApplyResponseBody {
            snapshot,
            outcome: decoded_outcome,
            ..
        }) = decoded
        else {
            panic!("expected work apply response");
        };
        assert_eq!(snapshot, WorkSnapshotDto::default());
        assert_eq!(decoded_outcome, outcome);

        let query_response = ClientResponse::WorkQueryResult(WorkQueryResponseBody {
            id: "work-2".into(),
            snapshot: WorkSnapshotDto::default(),
        });
        let encoded = serde_json::to_string(&query_response).expect("serialize query response");
        let decoded: ClientResponse = serde_json::from_str(&encoded).expect("deserialize response");
        assert!(matches!(decoded, ClientResponse::WorkQueryResult(_)));

        let unknown = r#"{"type":"work_apply","id":"w","session_id":"s","context":{},"operation":{"op":"start","goal":"g","unknown":true}}"#;
        assert!(serde_json::from_str::<ClientRequest>(unknown).is_err());

        let unknown_snapshot =
            r#"{"revision":0,"stack":[],"works":[],"entries":[],"unknown":true}"#;
        assert!(serde_json::from_str::<WorkSnapshotDto>(unknown_snapshot).is_err());

        for invalid in [
            r#"{"type":"work_apply","id":"w","session_id":"s","context":{},"operation":{"op":"start","goal":""}}"#,
            r#"{"type":"work_apply","id":"w","session_id":"s","context":{},"operation":{"op":"focus","text":"bad\u0000text"}}"#,
            r#"{"type":"work_apply","id":"w","session_id":"s","context":{},"operation":{"op":"switch","work_id":0}}"#,
        ] {
            assert!(serde_json::from_str::<ClientRequest>(invalid).is_err());
        }
        let oversized = serde_json::json!({
            "type": "work_apply",
            "id": "w",
            "session_id": "s",
            "context": {},
            "operation": {"op": "defer", "text": "x".repeat(WORK_TEXT_MAX_BYTES + 1)},
        });
        assert!(serde_json::from_value::<ClientRequest>(oversized).is_err());

        for unknown_response in [
            r#"{"type":"work_apply_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[],"entries":[]},"outcome":{"kind":"start"},"unknown":true}"#,
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[],"entries":[]},"unknown":true}"#,
        ] {
            assert!(serde_json::from_str::<ClientResponse>(unknown_response).is_err());
        }

        for zero_id_response in [
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"active_work_id":0,"stack":[],"works":[],"entries":[]}}"#,
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[0],"works":[],"entries":[]}}"#,
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[{"id":0,"title":"title","goal":"goal","status":"deferred","created_at_ms":1,"updated_at_ms":1}],"entries":[]}}"#,
            r#"{"type":"work_apply_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[],"entries":[]},"outcome":{"kind":"start","work_id":0}}"#,
        ] {
            assert!(serde_json::from_str::<ClientResponse>(zero_id_response).is_err());
        }

        for invalid_snapshot in [
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[{"id":1,"title":"","goal":"goal","status":"active","created_at_ms":1,"updated_at_ms":1}],"entries":[]}}"#,
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[{"id":1,"title":"title","goal":"   ","status":"active","created_at_ms":1,"updated_at_ms":1}],"entries":[]}}"#,
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[{"id":1,"title":"title","goal":"goal","status":"active","created_at_ms":1,"updated_at_ms":1,"focus":""}],"entries":[]}}"#,
            r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[{"id":1,"title":"title","goal":"goal","status":"active","created_at_ms":1,"updated_at_ms":1}],"entries":[{"id":1,"work_id":1,"kind":"note","text":"bad\u0000text","created_at_ms":1}]}}"#,
        ] {
            assert!(
                serde_json::from_str::<ClientResponse>(invalid_snapshot).is_err(),
                "expected invalid snapshot: {invalid_snapshot}"
            );
        }

        let valid_optional_focus = r#"{"type":"work_query_result","id":"w","snapshot":{"revision":0,"stack":[],"works":[{"id":1,"title":"title","goal":"goal","status":"active","created_at_ms":1,"updated_at_ms":1}],"entries":[]}}"#;
        assert!(serde_json::from_str::<ClientResponse>(valid_optional_focus).is_ok());
    }
}
