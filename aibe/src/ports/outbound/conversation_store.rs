use aibe_protocol::{ProtocolMessage, ProtocolMessageOut, RouteKind, RoutePlan};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationIndexEntry {
    pub session_id: String,
    pub conversation_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_kind: Option<RouteKind>,
    pub route_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recent_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSnapshot {
    pub session_id: String,
    pub conversation_id: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_plan: Option<RoutePlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<ProtocolMessage>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConversationStoreError {
    #[error("conversation store write failed: {0}")]
    Write(String),
    #[error("conversation store read failed: {0}")]
    Read(String),
    #[error("conversation not found: {0}")]
    NotFound(String),
}

pub trait ConversationStore: Send + Sync {
    fn ensure_conversation(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
    ) -> Result<(), ConversationStoreError>;

    fn upsert_route_plan(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
        plan: &RoutePlan,
        recent_summary: Option<String>,
    ) -> Result<(), ConversationStoreError>;

    fn record_turn(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
        request_messages: &[ProtocolMessage],
        assistant_message: &ProtocolMessageOut,
        route_plan: Option<&RoutePlan>,
    ) -> Result<(), ConversationStoreError>;

    fn load_snapshot(
        &self,
        session_id: &str,
        conversation_id: &str,
    ) -> Result<Option<ConversationSnapshot>, ConversationStoreError>;

    fn load_recent_summary(
        &self,
        session_id: &str,
        conversation_id: Option<&str>,
    ) -> Result<Option<String>, ConversationStoreError>;

    fn latest_conversation_id(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, ConversationStoreError>;
}
