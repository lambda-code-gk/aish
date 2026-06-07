//! `AI_SESSION_ID` ごとの conversation store。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use aibe_protocol::{ProtocolMessage, ProtocolMessageOut, RoutePlan};

use crate::ports::outbound::{
    ConversationIndexEntry, ConversationSnapshot, ConversationStore as ConversationStorePort,
    ConversationStoreError,
};

#[derive(Debug, Clone)]
pub struct ConversationStore {
    root: PathBuf,
}

impl ConversationStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".local/share/aibe/conversations")
    }

    fn session_root(&self, session_id: &str) -> PathBuf {
        self.root.join(session_id)
    }

    fn index_path(&self, session_id: &str) -> PathBuf {
        self.session_root(session_id).join("index.jsonl")
    }

    fn conversations_dir(&self, session_id: &str) -> PathBuf {
        self.session_root(session_id).join("conversations")
    }

    fn conversation_path(&self, session_id: &str, conversation_id: &str) -> PathBuf {
        self.conversations_dir(session_id)
            .join(format!("{conversation_id}.json"))
    }

    fn lock_path(&self, session_id: &str) -> PathBuf {
        self.session_root(session_id).join(".lock")
    }

    fn ensure_layout(&self, session_id: &str) -> Result<(), ConversationStoreError> {
        create_dir_0700(&self.root).map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        create_dir_0700(&self.session_root(session_id))
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        create_dir_0700(&self.conversations_dir(session_id))
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        Ok(())
    }

    fn with_lock<T>(
        &self,
        session_id: &str,
        f: impl FnOnce() -> Result<T, ConversationStoreError>,
    ) -> Result<T, ConversationStoreError> {
        self.ensure_layout(session_id)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(self.lock_path(session_id))
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        let guard = SessionLock::acquire(lock)?;
        let result = f();
        drop(guard);
        result
    }

    pub fn ensure_conversation(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
    ) -> Result<(), ConversationStoreError> {
        self.with_lock(session_id, || {
            let mut snapshot = self
                .load_snapshot_locked(session_id, conversation_id)?
                .unwrap_or_else(|| ConversationSnapshot {
                    session_id: session_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    created_at_ms,
                    updated_at_ms: created_at_ms,
                    route_plan: None,
                    summary: None,
                    messages: vec![],
                });
            snapshot.updated_at_ms = snapshot.updated_at_ms.max(created_at_ms);
            self.write_snapshot_locked(session_id, &snapshot)?;
            self.update_index_locked(
                session_id,
                ConversationIndexEntry {
                    session_id: session_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    created_at_ms: snapshot.created_at_ms,
                    updated_at_ms: snapshot.updated_at_ms,
                    route_kind: snapshot.route_plan.as_ref().map(|p| p.route_kind),
                    route_reason: snapshot
                        .route_plan
                        .as_ref()
                        .map(|p| redact_route_reason(&p.route_reason))
                        .unwrap_or_default(),
                    recent_summary: snapshot.summary.clone(),
                },
            )?;
            Ok(())
        })
    }

    pub fn upsert_route_plan(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
        plan: &RoutePlan,
        recent_summary: Option<String>,
    ) -> Result<(), ConversationStoreError> {
        self.with_lock(session_id, || {
            let mut snapshot = self
                .load_snapshot_locked(session_id, conversation_id)?
                .unwrap_or_else(|| ConversationSnapshot {
                    session_id: session_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    created_at_ms,
                    updated_at_ms: created_at_ms,
                    route_plan: None,
                    summary: None,
                    messages: vec![],
                });
            snapshot.route_plan = Some(plan.clone());
            snapshot.summary = recent_summary.clone();
            snapshot.updated_at_ms = created_at_ms.max(snapshot.updated_at_ms);
            self.write_snapshot_locked(session_id, &snapshot)?;
            self.update_index_locked(
                session_id,
                ConversationIndexEntry {
                    session_id: session_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    created_at_ms: snapshot.created_at_ms,
                    updated_at_ms: snapshot.updated_at_ms,
                    route_kind: Some(plan.route_kind),
                    route_reason: redact_route_reason(&plan.route_reason),
                    recent_summary,
                },
            )?;
            Ok(())
        })
    }

    pub fn record_turn(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
        request_messages: &[ProtocolMessage],
        assistant_message: &ProtocolMessageOut,
        route_plan: Option<&RoutePlan>,
    ) -> Result<(), ConversationStoreError> {
        self.with_lock(session_id, || {
            let mut snapshot = self
                .load_snapshot_locked(session_id, conversation_id)?
                .unwrap_or_else(|| ConversationSnapshot {
                    session_id: session_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    created_at_ms,
                    updated_at_ms: created_at_ms,
                    route_plan: route_plan.cloned(),
                    summary: None,
                    messages: vec![],
                });
            snapshot.messages = request_messages.to_vec();
            snapshot.messages.push(ProtocolMessage {
                role: assistant_message.role.clone(),
                content: assistant_message.content.clone(),
            });
            snapshot.summary = Some(build_recent_summary(&snapshot.messages));
            if let Some(route_plan) = route_plan {
                snapshot.route_plan = Some(route_plan.clone());
            }
            snapshot.updated_at_ms = created_at_ms.max(snapshot.updated_at_ms);
            self.write_snapshot_locked(session_id, &snapshot)?;
            self.update_index_locked(
                session_id,
                ConversationIndexEntry {
                    session_id: session_id.to_string(),
                    conversation_id: conversation_id.to_string(),
                    created_at_ms: snapshot.created_at_ms,
                    updated_at_ms: snapshot.updated_at_ms,
                    route_kind: snapshot.route_plan.as_ref().map(|p| p.route_kind),
                    route_reason: snapshot
                        .route_plan
                        .as_ref()
                        .map(|p| redact_route_reason(&p.route_reason))
                        .unwrap_or_default(),
                    recent_summary: snapshot.summary.clone(),
                },
            )?;
            Ok(())
        })
    }

    pub fn load_snapshot(
        &self,
        session_id: &str,
        conversation_id: &str,
    ) -> Result<Option<ConversationSnapshot>, ConversationStoreError> {
        self.with_lock(session_id, || {
            self.load_snapshot_locked(session_id, conversation_id)
        })
    }

    pub fn load_recent_summary(
        &self,
        session_id: &str,
        conversation_id: Option<&str>,
    ) -> Result<Option<String>, ConversationStoreError> {
        self.with_lock(session_id, || {
            if let Some(conversation_id) = conversation_id {
                let snapshot = self.load_snapshot_locked(session_id, conversation_id)?;
                return Ok(snapshot.and_then(|s| s.summary));
            }
            let latest = self.latest_index_entry_locked(session_id)?;
            Ok(latest.and_then(|entry| entry.recent_summary))
        })
    }

    pub fn latest_conversation_id(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, ConversationStoreError> {
        self.with_lock(session_id, || {
            Ok(self
                .latest_index_entry_locked(session_id)?
                .map(|entry| entry.conversation_id))
        })
    }

    fn latest_index_entry_locked(
        &self,
        session_id: &str,
    ) -> Result<Option<ConversationIndexEntry>, ConversationStoreError> {
        let mut entries = self.read_index_locked(session_id)?;
        entries.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| b.conversation_id.cmp(&a.conversation_id))
        });
        Ok(entries.into_iter().next())
    }

    fn read_index_locked(
        &self,
        session_id: &str,
    ) -> Result<Vec<ConversationIndexEntry>, ConversationStoreError> {
        let path = self.index_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path).map_err(|e| ConversationStoreError::Read(e.to_string()))?;
        let mut entries = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|e| ConversationStoreError::Read(e.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: ConversationIndexEntry = serde_json::from_str(&line)
                .map_err(|e| ConversationStoreError::Read(e.to_string()))?;
            entries.push(entry);
        }
        Ok(entries)
    }

    fn update_index_locked(
        &self,
        session_id: &str,
        entry: ConversationIndexEntry,
    ) -> Result<(), ConversationStoreError> {
        let mut map: HashMap<String, ConversationIndexEntry> = self
            .read_index_locked(session_id)?
            .into_iter()
            .map(|entry| (entry.conversation_id.clone(), entry))
            .collect();
        map.insert(entry.conversation_id.clone(), entry);
        let mut entries: Vec<_> = map.into_values().collect();
        entries.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| b.conversation_id.cmp(&a.conversation_id))
        });
        let index_path = self.index_path(session_id);
        let temp_path = index_path.with_extension("jsonl.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        for entry in entries {
            let line = serde_json::to_string(&entry)
                .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
            writeln!(file, "{line}").map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        }
        drop(file);
        set_permissions_0600(&temp_path)
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        fs::rename(&temp_path, &index_path)
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        set_permissions_0600(&index_path)
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        Ok(())
    }

    fn load_snapshot_locked(
        &self,
        session_id: &str,
        conversation_id: &str,
    ) -> Result<Option<ConversationSnapshot>, ConversationStoreError> {
        let path = self.conversation_path(session_id, conversation_id);
        if !path.exists() {
            return Ok(None);
        }
        let raw =
            fs::read_to_string(&path).map_err(|e| ConversationStoreError::Read(e.to_string()))?;
        let snapshot: ConversationSnapshot = serde_json::from_str(raw.trim())
            .map_err(|e| ConversationStoreError::Read(e.to_string()))?;
        Ok(Some(snapshot))
    }

    fn write_snapshot_locked(
        &self,
        session_id: &str,
        snapshot: &ConversationSnapshot,
    ) -> Result<(), ConversationStoreError> {
        let path = self.conversation_path(session_id, &snapshot.conversation_id);
        let temp_path = path.with_extension("json.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        let raw = serde_json::to_string_pretty(snapshot)
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        writeln!(file, "{raw}").map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        drop(file);
        set_permissions_0600(&temp_path)
            .map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        fs::rename(&temp_path, &path).map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        set_permissions_0600(&path).map_err(|e| ConversationStoreError::Write(e.to_string()))?;
        Ok(())
    }
}

impl ConversationStorePort for ConversationStore {
    fn ensure_conversation(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
    ) -> Result<(), ConversationStoreError> {
        ConversationStore::ensure_conversation(self, session_id, conversation_id, created_at_ms)
    }

    fn upsert_route_plan(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
        plan: &RoutePlan,
        recent_summary: Option<String>,
    ) -> Result<(), ConversationStoreError> {
        ConversationStore::upsert_route_plan(
            self,
            session_id,
            conversation_id,
            created_at_ms,
            plan,
            recent_summary,
        )
    }

    fn record_turn(
        &self,
        session_id: &str,
        conversation_id: &str,
        created_at_ms: u64,
        request_messages: &[ProtocolMessage],
        assistant_message: &ProtocolMessageOut,
        route_plan: Option<&RoutePlan>,
    ) -> Result<(), ConversationStoreError> {
        ConversationStore::record_turn(
            self,
            session_id,
            conversation_id,
            created_at_ms,
            request_messages,
            assistant_message,
            route_plan,
        )
    }

    fn load_snapshot(
        &self,
        session_id: &str,
        conversation_id: &str,
    ) -> Result<Option<ConversationSnapshot>, ConversationStoreError> {
        ConversationStore::load_snapshot(self, session_id, conversation_id)
    }

    fn load_recent_summary(
        &self,
        session_id: &str,
        conversation_id: Option<&str>,
    ) -> Result<Option<String>, ConversationStoreError> {
        ConversationStore::load_recent_summary(self, session_id, conversation_id)
    }

    fn latest_conversation_id(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, ConversationStoreError> {
        ConversationStore::latest_conversation_id(self, session_id)
    }
}

fn build_recent_summary(messages: &[ProtocolMessage]) -> String {
    let mut parts = Vec::new();
    for message in messages.iter().rev().take(2).rev() {
        let content = truncate_text(&message.content, 160);
        parts.push(format!("{}: {}", message.role, content));
    }
    if parts.is_empty() {
        "empty".to_string()
    } else {
        truncate_text(&parts.join(" | "), 320)
    }
}

fn redact_route_reason(reason: &str) -> String {
    let masked = mask_absolute_paths(reason);
    truncate_text(&masked, 200)
}

fn mask_absolute_paths(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '/' {
            out.push_str("<path>");
            while let Some(next) = chars.peek() {
                if next.is_whitespace() || matches!(next, ',' | ';' | ')' | '(' | '"' | '\'') {
                    break;
                }
                let _ = chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

struct SessionLock(File);

impl SessionLock {
    fn acquire(file: File) -> Result<Self, ConversationStoreError> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if rc != 0 {
                return Err(ConversationStoreError::Write(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
        }
        Ok(Self(file))
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

fn create_dir_0700(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    set_permissions_0700(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_permissions_0600(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_permissions_0600(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_permissions_0700(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_permissions_0700(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_reason_is_redacted_and_truncated() {
        let got = redact_route_reason("/tmp/project/very/long/path should be hidden");
        assert!(!got.contains("project"));
        assert!(got.len() <= 200);
    }

    #[test]
    fn recent_summary_uses_last_two_messages() {
        let summary = build_recent_summary(&[
            ProtocolMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            ProtocolMessage {
                role: "assistant".into(),
                content: "world".into(),
            },
            ProtocolMessage {
                role: "user".into(),
                content: "again".into(),
            },
        ]);
        assert!(summary.contains("assistant: world"));
        assert!(summary.contains("user: again"));
    }

    #[test]
    fn snapshot_roundtrip() {
        let snapshot = ConversationSnapshot {
            session_id: "sess".into(),
            conversation_id: "conv".into(),
            created_at_ms: 1,
            updated_at_ms: 2,
            route_plan: None,
            summary: Some("summary".into()),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
        };
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let back: ConversationSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.conversation_id, "conv");
        assert_eq!(back.messages.len(), 1);
    }
}
