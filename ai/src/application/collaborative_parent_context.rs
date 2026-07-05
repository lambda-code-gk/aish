//! 親 handoff 作成時の enriched parent context。

use aibe_protocol::{
    ClientResponse, MemoryContext, MemoryQueryDto, ProtocolMessage, WorkSnapshotDto,
};

use crate::ports::outbound::MemoryClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichedParentHandoffContext {
    pub parent_goal_id: Option<String>,
    pub parent_goal: String,
    pub parent_request_summary: String,
    pub conversation_snapshot: String,
    pub conversation_summary: String,
    pub work_stage_and_plan: String,
}

pub fn build_conversation_snapshot(messages: &[ProtocolMessage], max_messages: usize) -> String {
    let start = messages.len().saturating_sub(max_messages);
    messages[start..]
        .iter()
        .map(|message| format!("[{}] {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn build_enriched_parent_handoff_context(
    messages: &[ProtocolMessage],
    first_user_message: Option<String>,
    work_snapshot: Option<&WorkSnapshotDto>,
) -> EnrichedParentHandoffContext {
    let conversation_snapshot = build_conversation_snapshot(messages, 12);
    let fallback_summary = first_user_message
        .clone()
        .unwrap_or_else(|| "Complete the parent task".into());
    let (parent_goal_id, parent_goal, work_stage_and_plan) = work_snapshot
        .map(work_context_from_snapshot)
        .unwrap_or((None, fallback_summary.clone(), String::new()));
    EnrichedParentHandoffContext {
        parent_goal_id,
        parent_goal: if parent_goal.is_empty() {
            fallback_summary.clone()
        } else {
            parent_goal
        },
        parent_request_summary: fallback_summary.clone(),
        conversation_snapshot: if conversation_snapshot.is_empty() {
            fallback_summary.clone()
        } else {
            conversation_snapshot
        },
        conversation_summary: first_user_message.unwrap_or(fallback_summary),
        work_stage_and_plan,
    }
}

fn work_context_from_snapshot(snapshot: &WorkSnapshotDto) -> (Option<String>, String, String) {
    let active_id = snapshot.active_work_id;
    let parent_goal_id = active_id.map(|id| id.to_string());
    let active = active_id.and_then(|id| snapshot.works.iter().find(|work| work.id == id));
    let parent_goal = active
        .map(|work| {
            if work.goal.is_empty() {
                work.title.clone()
            } else {
                work.goal.clone()
            }
        })
        .unwrap_or_default();
    let mut lines = Vec::new();
    if let Some(work) = active {
        lines.push(format!("Active work #{}: {}", work.id, work.title));
        if let Some(focus) = work.focus.as_deref().filter(|text| !text.is_empty()) {
            lines.push(format!("Focus: {focus}"));
        }
        if let Some(summary) = work.summary.as_deref().filter(|text| !text.is_empty()) {
            lines.push(format!("Summary: {summary}"));
        }
        if !work.goal.is_empty() {
            lines.push(format!("Goal: {}", work.goal));
        }
    }
    if snapshot.stack.len() > 1 {
        lines.push(format!("Work stack: {:?}", snapshot.stack));
    }
    let recent_entries: Vec<String> = snapshot
        .entries
        .iter()
        .rev()
        .take(5)
        .map(|entry| format!("{:?}: {}", entry.kind, entry.text))
        .collect();
    if !recent_entries.is_empty() {
        lines.push(format!("Recent entries:\n{}", recent_entries.join("\n")));
    }
    (parent_goal_id, parent_goal, lines.join("\n"))
}

pub fn query_work_snapshot(
    client: &dyn crate::ports::outbound::WorkClient,
    session_id: &str,
    memory_space_id: Option<&str>,
    cwd: Option<&str>,
) -> Option<WorkSnapshotDto> {
    let context = MemoryContext {
        cwd: cwd.map(str::to_string),
        memory_space_id: memory_space_id.map(str::to_string),
    };
    match client.work_query(session_id, &context) {
        Ok(ClientResponse::WorkQueryResult(body)) => Some(body.snapshot),
        _ => None,
    }
}

/// side agent system context 用に contextual memory の prompt block を取得する。
pub fn query_collaborative_memory_prompt_block(
    client: &dyn MemoryClient,
    session_id: &str,
    memory_space_id: Option<&str>,
    cwd: &str,
    user_query: Option<&str>,
) -> Option<String> {
    let context = MemoryContext {
        cwd: Some(cwd.to_string()),
        memory_space_id: memory_space_id.map(str::to_string),
    };
    let query = MemoryQueryDto {
        include_prompt_block: true,
        user_query: user_query.map(str::to_string),
        ..Default::default()
    };
    match client.memory_query(session_id, &context, query) {
        Ok(ClientResponse::MemoryQueryResult {
            prompt_block: Some(block),
            ..
        }) if !block.trim().is_empty() => Some(block),
        _ => None,
    }
}

/// checkpoint に保存された親 memory space ID を返す（旧 handoff は None）。
pub fn checkpoint_memory_space_id(checkpoint: &crate::domain::HandoffCheckpoint) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(&checkpoint.environment_metadata)
        .ok()
        .and_then(|value| {
            value
                .get("memory_space_id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .filter(|id| !id.is_empty())
}
