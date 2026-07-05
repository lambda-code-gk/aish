//! handoff checkpoint への durable tool lifecycle 同期。

use aibe_protocol::ExecutedToolCall;

use crate::domain::{
    finalize_running_tools, record_tool_running, sync_tool_executions_from_executed,
    RecoverableToolStatus,
};
use crate::ports::outbound::{CheckpointRepository, HandoffStoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumedHandoffSync {
    pub handoff_id: String,
    pub sync_start_tool_call_id: Option<String>,
    pub sync_end_before_tool_call_id: Option<String>,
}

pub fn record_handoff_tool_running<S: CheckpointRepository>(
    store: &S,
    handoff_id: &str,
    tool_call_id: &str,
    tool_name: &str,
) -> Result<(), HandoffStoreError> {
    let mut checkpoint = store.load_checkpoint(handoff_id)?;
    record_tool_running(&mut checkpoint, tool_call_id, tool_name);
    store.save_checkpoint(handoff_id, &checkpoint)
}

pub fn record_handoff_tool_requested<S: CheckpointRepository>(
    store: &S,
    handoff_id: &str,
    tool_call_id: &str,
    tool_name: &str,
) -> Result<(), HandoffStoreError> {
    let mut checkpoint = store.load_checkpoint(handoff_id)?;
    crate::domain::upsert_tool_execution(
        &mut checkpoint,
        crate::domain::RecoverableToolExecution {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            status: RecoverableToolStatus::Requested,
        },
    );
    store.save_checkpoint(handoff_id, &checkpoint)
}

pub fn record_handoff_tool_approved<S: CheckpointRepository>(
    store: &S,
    handoff_id: &str,
    tool_call_id: &str,
    tool_name: &str,
) -> Result<(), HandoffStoreError> {
    let mut checkpoint = store.load_checkpoint(handoff_id)?;
    crate::domain::upsert_tool_execution(
        &mut checkpoint,
        crate::domain::RecoverableToolExecution {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            status: RecoverableToolStatus::Approved,
        },
    );
    store.save_checkpoint(handoff_id, &checkpoint)
}

pub fn sync_handoff_tool_executions<S: CheckpointRepository>(
    store: &S,
    handoff_id: &str,
    executed: &[ExecutedToolCall],
) -> Result<(), HandoffStoreError> {
    if executed.is_empty() {
        return Ok(());
    }
    let mut checkpoint = store.load_checkpoint(handoff_id)?;
    sync_tool_executions_from_executed(&mut checkpoint, executed);
    store.save_checkpoint(handoff_id, &checkpoint)
}

pub fn finalize_handoff_running_tools<S: CheckpointRepository>(
    store: &S,
    handoff_id: &str,
    terminal: RecoverableToolStatus,
    completed_call_id: Option<&str>,
) -> Result<(), HandoffStoreError> {
    let mut checkpoint = store.load_checkpoint(handoff_id)?;
    finalize_running_tools(&mut checkpoint, terminal, completed_call_id);
    store.save_checkpoint(handoff_id, &checkpoint)
}

fn filter_tool_calls_for_handoff(
    calls: &[ExecutedToolCall],
    sync_start_tool_call_id: Option<&str>,
    sync_end_before_tool_call_id: Option<&str>,
) -> Vec<ExecutedToolCall> {
    let start_idx = match sync_start_tool_call_id {
        None => 0,
        Some(id) => match calls.iter().position(|call| call.id == id) {
            Some(idx) => idx,
            None => return vec![],
        },
    };
    let end_idx = match sync_end_before_tool_call_id {
        None => calls.len(),
        Some(id) => match calls.iter().position(|call| call.id == id) {
            Some(idx) => idx,
            None => return vec![],
        },
    };
    if start_idx >= end_idx {
        return vec![];
    }
    calls[start_idx..end_idx].to_vec()
}

/// 親 RESUMING_PARENT turn 終了時に checkpoint へ tool lifecycle を確定する。
pub fn finalize_parent_resume_tool_tracking<S: CheckpointRepository>(
    store: &S,
    sync: &ResumedHandoffSync,
    parent_succeeded: bool,
    tool_calls: Option<&[ExecutedToolCall]>,
) -> Result<(), HandoffStoreError> {
    let filtered = tool_calls.map(|calls| {
        filter_tool_calls_for_handoff(
            calls,
            sync.sync_start_tool_call_id.as_deref(),
            sync.sync_end_before_tool_call_id.as_deref(),
        )
    });
    if let Some(calls) = filtered.filter(|calls| !calls.is_empty()) {
        sync_handoff_tool_executions(store, &sync.handoff_id, &calls)
    } else if !parent_succeeded {
        finalize_handoff_running_tools(
            store,
            &sync.handoff_id,
            RecoverableToolStatus::Unknown,
            None,
        )
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_tool_calls_respects_handoff_boundaries() {
        let calls = vec![
            ExecutedToolCall::ok("a".into(), "one", serde_json::json!({}), "ok".into()),
            ExecutedToolCall::ok("b".into(), "two", serde_json::json!({}), "ok".into()),
            ExecutedToolCall::ok("c".into(), "three", serde_json::json!({}), "ok".into()),
        ];
        let filtered = filter_tool_calls_for_handoff(&calls, Some("b"), Some("c"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "b");
    }

    #[test]
    fn filter_tool_calls_returns_empty_when_start_boundary_missing() {
        let calls = vec![ExecutedToolCall::ok(
            "a".into(),
            "one",
            serde_json::json!({}),
            "ok".into(),
        )];
        assert!(filter_tool_calls_for_handoff(&calls, Some("missing"), None).is_empty());
    }

    #[test]
    fn filter_tool_calls_returns_empty_when_end_boundary_missing() {
        let calls = vec![ExecutedToolCall::ok(
            "a".into(),
            "one",
            serde_json::json!({}),
            "ok".into(),
        )];
        assert!(filter_tool_calls_for_handoff(&calls, Some("a"), Some("missing")).is_empty());
    }

    #[test]
    fn filter_tool_calls_returns_empty_when_boundaries_reversed() {
        let calls = vec![
            ExecutedToolCall::ok("a".into(), "one", serde_json::json!({}), "ok".into()),
            ExecutedToolCall::ok("b".into(), "two", serde_json::json!({}), "ok".into()),
        ];
        assert!(filter_tool_calls_for_handoff(&calls, Some("b"), Some("a")).is_empty());
    }
}
