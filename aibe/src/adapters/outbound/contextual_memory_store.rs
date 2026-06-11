//! session 配下 JSONL contextual memory store。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use aibe_protocol::{MemoryOperationDto, MemoryQueryDto, MemoryScopeDto, MemoryStatusDto};

use crate::domain::resolve_entries_for_prompt;
use crate::domain::{
    is_standard_kind, validate_kind, validate_standard_kind_operation, validate_text, MemoryBlock,
    MemoryEntry, MemoryInjectPolicy, MemoryScope, MemoryStatus, MemoryValidationError,
    ProjectKeyError, STANDARD_KIND_IDEA,
};
use crate::ports::outbound::{ContextualMemoryStore, ContextualMemoryStoreError};

/// テスト・既存経路向けの no-op store（常に空）。
#[derive(Debug, Default, Clone)]
pub struct EmptyContextualMemoryStore;

impl ContextualMemoryStore for EmptyContextualMemoryStore {
    fn apply(
        &self,
        _session_id: &str,
        _cwd: Option<&Path>,
        _operation: &MemoryOperationDto,
        _now_ms: u64,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        Ok(vec![])
    }

    fn query(
        &self,
        _session_id: &str,
        _cwd: Option<&Path>,
        _query: &MemoryQueryDto,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        Ok(vec![])
    }

    fn resolve_for_prompt(
        &self,
        _session_id: &str,
        _cwd: Option<&Path>,
        _user_query: &str,
        _budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        Ok(MemoryBlock {
            content: String::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct FilesystemContextualMemoryStore {
    root: PathBuf,
}

impl FilesystemContextualMemoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn with_conversation_root(conversation_root: PathBuf) -> Self {
        Self::new(conversation_root)
    }

    fn session_root(&self, session_id: &str) -> PathBuf {
        self.root.join(session_id)
    }

    fn memory_dir(&self, session_id: &str) -> PathBuf {
        self.session_root(session_id).join("memory")
    }

    fn events_path(&self, session_id: &str) -> PathBuf {
        self.memory_dir(session_id).join("events.jsonl")
    }

    fn lock_path(&self, session_id: &str) -> PathBuf {
        self.session_root(session_id).join(".lock")
    }

    fn ensure_layout(&self, session_id: &str) -> Result<(), ContextualMemoryStoreError> {
        create_dir_0700(&self.root).map_err(io_err)?;
        create_dir_0700(&self.session_root(session_id)).map_err(io_err)?;
        create_dir_0700(&self.memory_dir(session_id)).map_err(io_err)?;
        Ok(())
    }

    fn with_lock<T>(
        &self,
        session_id: &str,
        f: impl FnOnce() -> Result<T, ContextualMemoryStoreError>,
    ) -> Result<T, ContextualMemoryStoreError> {
        self.ensure_layout(session_id)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(self.lock_path(session_id))
            .map_err(io_err)?;
        let guard = SessionLock::acquire(lock)?;
        let result = f();
        drop(guard);
        result
    }

    fn load_entries(
        &self,
        session_id: &str,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        let path = self.events_path(session_id);
        if !path.exists() {
            return Ok(vec![]);
        }
        let file = File::open(&path).map_err(io_err)?;
        set_permissions_0600(&path).ok();
        let reader = BufReader::new(file);
        let mut map: HashMap<String, MemoryEntry> = HashMap::new();
        for line in reader.lines() {
            let line = line.map_err(io_err)?;
            if line.trim().is_empty() {
                continue;
            }
            let event: StoredEvent =
                serde_json::from_str(&line).map_err(|e| io_err(e.to_string()))?;
            apply_event(&mut map, event);
        }
        Ok(map.into_values().collect())
    }

    fn append_event(
        &self,
        session_id: &str,
        event: StoredEvent,
    ) -> Result<(), ContextualMemoryStoreError> {
        let path = self.events_path(session_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io_err)?;
        set_permissions_0600(&path).ok();
        let line = serde_json::to_string(&event).map_err(|e| io_err(e.to_string()))?;
        writeln!(file, "{line}").map_err(io_err)?;
        Ok(())
    }

    fn project_key_from_cwd(
        cwd: Option<&Path>,
    ) -> Result<Option<String>, ContextualMemoryStoreError> {
        let Some(cwd) = cwd else {
            return Ok(None);
        };
        resolve_project_key(cwd).map(Some)
    }
}

impl ContextualMemoryStore for FilesystemContextualMemoryStore {
    fn apply(
        &self,
        session_id: &str,
        cwd: Option<&Path>,
        operation: &MemoryOperationDto,
        now_ms: u64,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        if session_id.is_empty() {
            return Err(ContextualMemoryStoreError::Validation(
                MemoryValidationError::EmptySessionId,
            ));
        }
        let project_key = Self::project_key_from_cwd(cwd)?;
        self.with_lock(session_id, || match operation {
            MemoryOperationDto::Add {
                kind,
                scope,
                inject,
                status,
                text,
                make_active,
            } => {
                validate_kind(kind)?;
                validate_text(text)?;
                if is_standard_kind(kind) {
                    validate_standard_kind_operation(kind, *scope, *inject, *status)?;
                }
                let scope = MemoryScope::try_from(*scope).map_err(|_| {
                    ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                        "scope".into(),
                    ))
                })?;
                if scope == MemoryScope::Project && project_key.is_none() {
                    return Err(ContextualMemoryStoreError::Validation(
                        MemoryValidationError::MissingCwdForProjectScope,
                    ));
                }
                let inject = MemoryInjectPolicy::try_from(*inject).map_err(|_| {
                    ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                        "inject".into(),
                    ))
                })?;
                let client_status = MemoryStatus::try_from(*status).map_err(|_| {
                    ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                        "status".into(),
                    ))
                })?;
                let final_status = if *make_active {
                    MemoryStatus::Active
                } else {
                    client_status
                };

                let mut affected = Vec::new();
                if *make_active {
                    let mut entries = self.load_entries(session_id)?;
                    for entry in entries.iter_mut() {
                        if entry.kind == *kind
                            && entry.scope == scope
                            && entry.status == MemoryStatus::Active
                            && scope_matches_project_key(scope, &entry.project_key, &project_key)
                        {
                            entry.status = MemoryStatus::Inactive;
                            entry.updated_at_ms = now_ms;
                            entry.version += 1;
                            self.append_event(
                                session_id,
                                StoredEvent::StatusChanged {
                                    id: entry.id.clone(),
                                    status: MemoryStatus::Inactive,
                                    updated_at_ms: now_ms,
                                    version: entry.version,
                                },
                            )?;
                            affected.push(entry.clone());
                        }
                    }
                }

                let entries = self.load_entries(session_id)?;
                let id = next_id(&entries, session_id);
                let version = 1;
                let new_entry = MemoryEntry {
                    id: id.clone(),
                    session_id: session_id.to_string(),
                    kind: kind.clone(),
                    scope,
                    inject,
                    status: final_status,
                    text: text.clone(),
                    project_key: if scope == MemoryScope::Project {
                        project_key.clone()
                    } else {
                        None
                    },
                    created_at_ms: now_ms,
                    updated_at_ms: now_ms,
                    version,
                };
                self.append_event(
                    session_id,
                    StoredEvent::Added {
                        entry: new_entry.clone(),
                    },
                )?;
                affected.push(new_entry);
                Ok(affected)
            }
            MemoryOperationDto::ClearActive { kind, scope } => {
                validate_kind(kind)?;
                let scope = MemoryScope::try_from(*scope).map_err(|_| {
                    ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                        "scope".into(),
                    ))
                })?;
                if scope == MemoryScope::Project && project_key.is_none() {
                    return Err(ContextualMemoryStoreError::Validation(
                        MemoryValidationError::MissingCwdForProjectScope,
                    ));
                }
                let mut entries = self.load_entries(session_id)?;
                let mut affected = Vec::new();
                for entry in entries.iter_mut() {
                    let status_match = if *kind == STANDARD_KIND_IDEA {
                        entry.status == MemoryStatus::Open
                    } else {
                        entry.status == MemoryStatus::Active
                    };
                    if entry.kind == *kind
                        && entry.scope == scope
                        && status_match
                        && scope_matches_project_key(scope, &entry.project_key, &project_key)
                    {
                        let new_status = if *kind == STANDARD_KIND_IDEA {
                            MemoryStatus::Archived
                        } else {
                            MemoryStatus::Inactive
                        };
                        entry.status = new_status;
                        entry.updated_at_ms = now_ms;
                        entry.version += 1;
                        self.append_event(
                            session_id,
                            StoredEvent::StatusChanged {
                                id: entry.id.clone(),
                                status: new_status,
                                updated_at_ms: now_ms,
                                version: entry.version,
                            },
                        )?;
                        affected.push(entry.clone());
                    }
                }
                Ok(affected)
            }
            MemoryOperationDto::Archive {
                id,
                expected_version,
            } => {
                let mut entries = self.load_entries(session_id)?;
                let entry = entries
                    .iter_mut()
                    .find(|e| e.id == *id)
                    .ok_or_else(|| ContextualMemoryStoreError::NotFound(id.clone()))?;
                if let Some(expected) = expected_version {
                    if entry.version != *expected {
                        return Err(ContextualMemoryStoreError::Validation(
                            MemoryValidationError::VersionConflict,
                        ));
                    }
                }
                entry.status = MemoryStatus::Archived;
                entry.updated_at_ms = now_ms;
                entry.version += 1;
                self.append_event(
                    session_id,
                    StoredEvent::StatusChanged {
                        id: entry.id.clone(),
                        status: MemoryStatus::Archived,
                        updated_at_ms: now_ms,
                        version: entry.version,
                    },
                )?;
                Ok(vec![entry.clone()])
            }
        })
    }

    fn query(
        &self,
        session_id: &str,
        cwd: Option<&Path>,
        query: &MemoryQueryDto,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        if session_id.is_empty() {
            return Err(ContextualMemoryStoreError::Validation(
                MemoryValidationError::EmptySessionId,
            ));
        }
        let project_key = Self::project_key_from_cwd(cwd)?;
        self.with_lock(session_id, || {
            let mut entries = self.load_entries(session_id)?;
            entries.retain(|e| filter_entry(e, query, project_key.as_deref()));
            entries.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
            if let Some(limit) = query.limit {
                entries.truncate(limit as usize);
            }
            Ok(entries)
        })
    }

    fn resolve_for_prompt(
        &self,
        session_id: &str,
        cwd: Option<&Path>,
        user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        if session_id.is_empty() {
            return Ok(MemoryBlock {
                content: String::new(),
            });
        }
        let project_key = Self::project_key_from_cwd(cwd)?;
        let entries = self.with_lock(session_id, || self.load_entries(session_id))?;
        Ok(resolve_entries_for_prompt(
            &entries,
            project_key.as_deref(),
            user_query,
            budget_bytes,
        ))
    }
}

fn resolve_project_key(cwd: &Path) -> Result<String, ContextualMemoryStoreError> {
    let abs = cwd.canonicalize().map_err(|e| {
        ContextualMemoryStoreError::ProjectKey(ProjectKeyError::Resolve(e.to_string()))
    })?;
    let key = find_git_root(&abs).unwrap_or(abs);
    let canonical = key.canonicalize().map_err(|e| {
        ContextualMemoryStoreError::ProjectKey(ProjectKeyError::Resolve(e.to_string()))
    })?;
    Ok(canonical.to_string_lossy().into_owned())
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn filter_entry(entry: &MemoryEntry, query: &MemoryQueryDto, project_key: Option<&str>) -> bool {
    if let Some(kind) = query.kind.as_deref() {
        if entry.kind != kind {
            return false;
        }
    }
    if let Some(scope) = &query.scope {
        if &MemoryScopeDto::from(entry.scope) != scope {
            return false;
        }
    }
    if let Some(status) = &query.status {
        if &MemoryStatusDto::from(entry.status) != status {
            return false;
        }
    }
    if query.active_only && entry.status != MemoryStatus::Active {
        return false;
    }
    if !query.include_archived && entry.status == MemoryStatus::Archived {
        return false;
    }
    scope_matches_project_key(
        entry.scope,
        &entry.project_key,
        &project_key.map(String::from),
    )
}

/// project scope は `project_key` 一致が必要。session / global は同一 session 内で kind + scope のみ。
fn scope_matches_project_key(
    scope: MemoryScope,
    entry_pk: &Option<String>,
    query_pk: &Option<String>,
) -> bool {
    match scope {
        MemoryScope::Project => project_keys_match(entry_pk, query_pk),
        MemoryScope::Session | MemoryScope::Global => true,
    }
}

fn project_keys_match(entry_pk: &Option<String>, query_pk: &Option<String>) -> bool {
    match (entry_pk, query_pk) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    }
}

fn next_id(entries: &[MemoryEntry], session_id: &str) -> String {
    let seq = entries.len() + 1;
    format!("mem_{session_id}_{seq}")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum StoredEvent {
    Added {
        entry: MemoryEntry,
    },
    StatusChanged {
        id: String,
        status: MemoryStatus,
        updated_at_ms: u64,
        version: u64,
    },
}

fn apply_event(map: &mut HashMap<String, MemoryEntry>, event: StoredEvent) {
    match event {
        StoredEvent::Added { entry } => {
            map.insert(entry.id.clone(), entry);
        }
        StoredEvent::StatusChanged {
            id,
            status,
            updated_at_ms,
            version,
        } => {
            if let Some(entry) = map.get_mut(&id) {
                entry.status = status;
                entry.updated_at_ms = updated_at_ms;
                entry.version = version;
            }
        }
    }
}

fn io_err(e: impl ToString) -> ContextualMemoryStoreError {
    ContextualMemoryStoreError::Io(e.to_string())
}

struct SessionLock(File);

impl SessionLock {
    fn acquire(file: File) -> Result<Self, ContextualMemoryStoreError> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if rc != 0 {
                return Err(io_err("failed to acquire session lock"));
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
    set_permissions_0700(path)
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
    use crate::domain::{STANDARD_KIND_GOAL, STANDARD_KIND_NOW};
    use aibe_protocol::{MemoryInjectPolicyDto, MemoryScopeDto, MemoryStatusDto};
    use tempfile::TempDir;

    fn store() -> (FilesystemContextualMemoryStore, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        (
            FilesystemContextualMemoryStore::new(dir.path().to_path_buf()),
            dir,
        )
    }

    fn goal_add(text: &str) -> MemoryOperationDto {
        MemoryOperationDto::Add {
            kind: STANDARD_KIND_GOAL.into(),
            scope: MemoryScopeDto::Project,
            inject: MemoryInjectPolicyDto::Pinned,
            status: MemoryStatusDto::Active,
            text: text.into(),
            make_active: true,
        }
    }

    #[test]
    fn goal_add_creates_active_goal() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        let entries = store
            .apply("sess", Some(&cwd), &goal_add("first"), 1)
            .expect("apply");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, MemoryStatus::Active);
    }

    #[test]
    fn goal_add_twice_inactivates_old() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        store
            .apply("sess", Some(&cwd), &goal_add("first"), 1)
            .expect("apply");
        let second = store
            .apply("sess", Some(&cwd), &goal_add("second"), 2)
            .expect("apply");
        assert_eq!(second.last().expect("new").text, "second");
        let all = store.load_entries("sess").expect("load");
        let active: Vec<_> = all
            .iter()
            .filter(|e| e.kind == STANDARD_KIND_GOAL && e.status == MemoryStatus::Active)
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].text, "second");
    }

    fn now_add(text: &str) -> MemoryOperationDto {
        MemoryOperationDto::Add {
            kind: STANDARD_KIND_NOW.into(),
            scope: MemoryScopeDto::Session,
            inject: MemoryInjectPolicyDto::Pinned,
            status: MemoryStatusDto::Active,
            text: text.into(),
            make_active: true,
        }
    }

    #[test]
    fn now_add_twice_inactivates_old() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        store
            .apply("sess", Some(&cwd), &now_add("first"), 1)
            .expect("apply");
        let second = store
            .apply("sess", Some(&cwd), &now_add("second"), 2)
            .expect("apply");
        assert_eq!(second.last().expect("new").text, "second");
        let all = store.load_entries("sess").expect("load");
        let active: Vec<_> = all
            .iter()
            .filter(|e| e.kind == STANDARD_KIND_NOW && e.status == MemoryStatus::Active)
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].text, "second");
    }

    #[test]
    fn now_clear_inactivates_active() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        store
            .apply("sess", Some(&cwd), &now_add("focus"), 1)
            .expect("apply");
        let cleared = store
            .apply(
                "sess",
                Some(&cwd),
                &MemoryOperationDto::ClearActive {
                    kind: STANDARD_KIND_NOW.into(),
                    scope: MemoryScopeDto::Session,
                },
                2,
            )
            .expect("clear");
        assert_eq!(cleared.len(), 1);
        assert_eq!(cleared[0].status, MemoryStatus::Inactive);
        let all = store.load_entries("sess").expect("load");
        assert!(
            all.iter()
                .filter(|e| e.kind == STANDARD_KIND_NOW && e.status == MemoryStatus::Active)
                .count()
                == 0
        );
    }

    #[test]
    fn idea_add_keeps_multiple_open() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        let op = MemoryOperationDto::Add {
            kind: STANDARD_KIND_IDEA.into(),
            scope: MemoryScopeDto::Project,
            inject: MemoryInjectPolicyDto::OnDemand,
            status: MemoryStatusDto::Open,
            text: "a".into(),
            make_active: false,
        };
        store.apply("sess", Some(&cwd), &op, 1).expect("apply");
        let op2 = MemoryOperationDto::Add {
            kind: STANDARD_KIND_IDEA.into(),
            scope: MemoryScopeDto::Project,
            inject: MemoryInjectPolicyDto::OnDemand,
            status: MemoryStatusDto::Open,
            text: "b".into(),
            make_active: false,
        };
        store.apply("sess", Some(&cwd), &op2, 2).expect("apply");
        let ideas = store
            .query(
                "sess",
                Some(&cwd),
                &MemoryQueryDto {
                    kind: Some(STANDARD_KIND_IDEA.into()),
                    scope: None,
                    status: None,
                    active_only: false,
                    include_archived: false,
                    limit: None,
                    include_prompt_block: false,
                    user_query: None,
                },
            )
            .expect("query");
        assert_eq!(ideas.len(), 2);
    }
}
