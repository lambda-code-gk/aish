//! local history のユースケース。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::domain::{
    HistoryIndexEntry, HistoryIndexFilter, HistoryIndexView, HistoryMessage, HistoryPayload,
    HistoryRecordKind, HistoryRecordStatus, HistorySummary,
};
use crate::ports::outbound::{HistoryStore, HistoryStoreError};

#[derive(Debug, Clone)]
pub struct HistoryRecordInput {
    pub command: String,
    pub session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub preset: Option<String>,
    pub profile: Option<String>,
    pub shell_exec_approval: Option<String>,
    pub socket_path: String,
    pub request_kind: HistoryRecordKind,
    pub request_summary: HistorySummary,
    pub response_kind: HistoryRecordKind,
    pub response_summary: HistorySummary,
    pub status: HistoryRecordStatus,
}

#[derive(Debug, Clone)]
pub struct HistoryReplayInput {
    pub history_id: String,
    pub command: String,
    pub user_message: String,
    pub shell_log_tail: Option<String>,
    pub client_cwd: Option<String>,
    pub tools: Vec<String>,
    pub llm_profile: Option<String>,
    pub preset: Option<String>,
    pub session_id: Option<String>,
    pub conversation_id: Option<String>,
    pub shell_exec_approval: Option<String>,
    pub socket_path: String,
    pub log_tail_bytes: usize,
    pub request_messages: Vec<HistoryMessage>,
}

pub fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn next_history_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{:016x}{:08x}", current_time_ms(), seq)
}

pub fn build_summary(
    user_message: &str,
    shell_log_tail: Option<&str>,
    tools: &[String],
) -> HistorySummary {
    HistorySummary::new(format!(
        "user_message_len={} shell_log_tail_len={} tools={}",
        user_message.len(),
        shell_log_tail.map(|s| s.len()).unwrap_or(0),
        tools.len()
    ))
}

pub fn build_response_summary(
    assistant_message_len: usize,
    tool_calls: usize,
    error: Option<&str>,
) -> HistorySummary {
    match error {
        Some(err) => HistorySummary::new(format!(
            "error={} assistant_message_len={assistant_message_len} tool_calls={tool_calls}",
            err
        )),
        None => HistorySummary::new(format!(
            "assistant_message_len={assistant_message_len} tool_calls={tool_calls}"
        )),
    }
}

pub fn record_turn<S: HistoryStore>(
    store: &S,
    input: &HistoryRecordInput,
    payload: &HistoryReplayInput,
    history_max_entries: usize,
) -> Result<String, HistoryStoreError> {
    let history_id = payload.history_id.clone();
    let entry = HistoryIndexEntry {
        history_id: history_id.clone(),
        created_at_ms: current_time_ms(),
        command: input.command.clone(),
        session_id: input.session_id.clone(),
        conversation_id: input.conversation_id.clone(),
        preset: input.preset.clone(),
        profile: input.profile.clone(),
        shell_exec_approval: input.shell_exec_approval.clone(),
        socket_path: input.socket_path.clone(),
        request_kind: input.request_kind.clone(),
        request_summary: input.request_summary.clone(),
        response_kind: input.response_kind.clone(),
        response_summary: input.response_summary.clone(),
        status: input.status.clone(),
    };
    let payload = HistoryPayload {
        history_id: payload.history_id.clone(),
        command: payload.command.clone(),
        user_message: payload.user_message.clone(),
        shell_log_tail: payload.shell_log_tail.clone(),
        client_cwd: payload.client_cwd.clone(),
        tools: payload.tools.clone(),
        llm_profile: payload.llm_profile.clone(),
        preset: payload.preset.clone(),
        session_id: payload.session_id.clone(),
        conversation_id: payload.conversation_id.clone(),
        shell_exec_approval: payload.shell_exec_approval.clone(),
        socket_path: payload.socket_path.clone(),
        log_tail_bytes: payload.log_tail_bytes,
        request_messages: payload.request_messages.clone(),
    };
    store.append(&entry, &payload)?;
    if history_max_entries > 0 {
        let _ = store.prune_to_max(history_max_entries)?;
    }
    Ok(history_id)
}

pub fn list_history<S: HistoryStore>(
    store: &S,
    filter: HistoryIndexFilter,
) -> Result<Vec<HistoryIndexView>, HistoryStoreError> {
    let mut entries = store.list()?;
    if let Some(session_id) = filter.session_id.as_deref() {
        entries.retain(|entry| entry.session_id.as_deref() == Some(session_id));
    }
    if let Some(command) = filter.command.as_deref() {
        entries.retain(|entry| entry.command == command);
    }
    if let Some(status) = filter.status {
        entries.retain(|entry| entry.status == status);
    }
    entries.truncate(filter.limit);
    Ok(entries.iter().map(HistoryIndexView::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::ports::outbound::{HistoryStore, HistoryStoreError};

    #[derive(Default)]
    struct MemoryHistoryStore {
        entries: Mutex<Vec<HistoryIndexEntry>>,
        payloads: Mutex<HashMap<String, HistoryPayload>>,
    }

    impl HistoryStore for MemoryHistoryStore {
        fn append(
            &self,
            entry: &HistoryIndexEntry,
            payload: &HistoryPayload,
        ) -> Result<(), HistoryStoreError> {
            self.entries.lock().expect("lock").push(entry.clone());
            self.payloads
                .lock()
                .expect("lock")
                .insert(entry.history_id.clone(), payload.clone());
            Ok(())
        }

        fn list(&self) -> Result<Vec<HistoryIndexEntry>, HistoryStoreError> {
            Ok(self.entries.lock().expect("lock").clone())
        }

        fn load_payload(&self, history_id: &str) -> Result<HistoryPayload, HistoryStoreError> {
            self.payloads
                .lock()
                .expect("lock")
                .get(history_id)
                .cloned()
                .ok_or_else(|| HistoryStoreError::NotFound(history_id.to_string()))
        }

        fn prune_to_max(&self, max_entries: usize) -> Result<usize, HistoryStoreError> {
            if max_entries == 0 {
                return Ok(0);
            }
            let mut entries = self.entries.lock().expect("lock");
            if entries.len() <= max_entries {
                return Ok(0);
            }
            entries.sort_by(|a, b| {
                b.created_at_ms
                    .cmp(&a.created_at_ms)
                    .then_with(|| b.history_id.cmp(&a.history_id))
            });
            let drop_count = entries.len() - max_entries;
            let dropped: Vec<_> = entries.drain(max_entries..).collect();
            let mut payloads = self.payloads.lock().expect("lock");
            for entry in dropped {
                payloads.remove(&entry.history_id);
            }
            Ok(drop_count)
        }
    }

    #[test]
    fn build_summary_redacts_raw_text() {
        let summary = build_summary("hello", Some("tail"), &["read_file".into()]);
        assert!(summary.detail.contains("user_message_len=5"));
        assert!(!summary.detail.contains("hello"));
    }

    #[test]
    fn record_and_list_history_round_trip() {
        let store = MemoryHistoryStore::default();
        let history_id = next_history_id();
        let input = HistoryRecordInput {
            command: "ask".into(),
            session_id: Some("sess".into()),
            conversation_id: Some("conv".into()),
            preset: Some("fast".into()),
            profile: Some("fast".into()),
            shell_exec_approval: Some("ask".into()),
            socket_path: "/tmp/sock".into(),
            request_kind: HistoryRecordKind::Ask,
            request_summary: HistorySummary::new("request"),
            response_kind: HistoryRecordKind::Ask,
            response_summary: HistorySummary::new("response"),
            status: HistoryRecordStatus::Ok,
        };
        let payload = HistoryReplayInput {
            history_id: history_id.clone(),
            command: "ask".into(),
            user_message: "hello".into(),
            shell_log_tail: None,
            client_cwd: Some("/tmp".into()),
            tools: vec!["read_file".into()],
            llm_profile: Some("fast".into()),
            preset: Some("fast".into()),
            session_id: Some("sess".into()),
            conversation_id: Some("conv".into()),
            shell_exec_approval: Some("ask".into()),
            socket_path: "/tmp/sock".into(),
            log_tail_bytes: 16,
            request_messages: vec![HistoryMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
        };
        record_turn(&store, &input, &payload, 500).expect("record");
        let listed = list_history(
            &store,
            HistoryIndexFilter {
                session_id: Some("sess".into()),
                command: Some("ask".into()),
                status: Some(HistoryRecordStatus::Ok),
                limit: 10,
            },
        )
        .expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].history_id, history_id);
        assert_eq!(listed[0].conversation_id.as_deref(), Some("conv"));
        assert_eq!(listed[0].shell_exec_approval.as_deref(), Some("ask"));
    }
}
