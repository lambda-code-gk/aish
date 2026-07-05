//! handoff checkpoint への durable tool lifecycle 同期。

use aibe_protocol::ExecutedToolCall;

use crate::domain::{
    finalize_running_tools, record_tool_running, sync_tool_executions_from_executed,
    RecoverableToolStatus,
};
use crate::ports::outbound::{CheckpointRepository, HandoffStoreError};

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

/// 親 RESUMING_PARENT turn 終了時に checkpoint へ tool lifecycle を確定する。
pub fn finalize_parent_resume_tool_tracking<S: CheckpointRepository>(
    store: &S,
    handoff_id: &str,
    parent_succeeded: bool,
    tool_calls: Option<&[ExecutedToolCall]>,
) -> Result<(), HandoffStoreError> {
    if let Some(calls) = tool_calls.filter(|calls| !calls.is_empty()) {
        sync_handoff_tool_executions(store, handoff_id, calls)
    } else if !parent_succeeded {
        finalize_handoff_running_tools(store, handoff_id, RecoverableToolStatus::Cancelled, None)
    } else {
        Ok(())
    }
}
