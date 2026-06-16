//! memory space 配下 JSONL contextual memory store。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aibe_protocol::{
    is_valid_session_id, legacy_session_memory_space_id, MemoryOperationDto, MemoryQueryDto,
    MemoryScopeDto, MemoryStatusDto,
};

use crate::adapters::outbound::FilesystemMemoryKindRegistryLoader;
use crate::domain::resolve_entries_for_prompt;
use crate::domain::{
    validate_kind, validate_standard_kind_operation, validate_text, MemoryBlock, MemoryEntry,
    MemoryInjectPolicy, MemoryScope, MemoryStatus, MemoryValidationError, ProjectKeyError,
};
use crate::ports::outbound::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemoryConfig, MemoryKindRegistryLoader,
    MemoryStoreContext,
};

/// テスト・既存経路向けの no-op store（常に空）。
#[derive(Debug, Default, Clone)]
pub struct EmptyContextualMemoryStore;

impl ContextualMemoryStore for EmptyContextualMemoryStore {
    fn apply(
        &self,
        _ctx: &MemoryStoreContext<'_>,
        _operation: &MemoryOperationDto,
        _now_ms: u64,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        Ok(vec![])
    }

    fn query(
        &self,
        _ctx: &MemoryStoreContext<'_>,
        _query: &MemoryQueryDto,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        Ok(vec![])
    }

    fn resolve_for_prompt(
        &self,
        _ctx: &MemoryStoreContext<'_>,
        _user_query: &str,
        _budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        Ok(MemoryBlock {
            content: String::new(),
        })
    }

    fn resolve_for_prompt_explicit(
        &self,
        _ctx: &MemoryStoreContext<'_>,
        _user_query: &str,
        _budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        Ok(MemoryBlock {
            content: String::new(),
        })
    }
}

#[derive(Clone)]
pub struct FilesystemContextualMemoryStore {
    aibe_root: PathBuf,
    registry_loader: Arc<dyn MemoryKindRegistryLoader>,
}

impl FilesystemContextualMemoryStore {
    pub fn new(aibe_root: PathBuf) -> Self {
        Self::with_memory_config(aibe_root, MemoryConfig::default())
    }

    pub fn with_memory_config(aibe_root: PathBuf, memory_config: MemoryConfig) -> Self {
        let loader = Arc::new(FilesystemMemoryKindRegistryLoader::with_memory_config(
            aibe_root.clone(),
            memory_config,
        ));
        Self {
            aibe_root,
            registry_loader: loader,
        }
    }

    pub fn with_registry_loader(
        aibe_root: PathBuf,
        registry_loader: Arc<dyn MemoryKindRegistryLoader>,
    ) -> Self {
        Self {
            aibe_root,
            registry_loader,
        }
    }

    pub fn registry_loader(&self) -> Arc<dyn MemoryKindRegistryLoader> {
        Arc::clone(&self.registry_loader)
    }

    pub fn with_aibe_root(aibe_root: PathBuf) -> Self {
        Self::new(aibe_root)
    }

    pub fn with_conversation_root(conversation_root: PathBuf) -> Self {
        Self::with_conversation_root_and_config(conversation_root, MemoryConfig::default())
    }

    pub fn with_conversation_root_and_config(
        conversation_root: PathBuf,
        memory_config: MemoryConfig,
    ) -> Self {
        let aibe_root = conversation_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(conversation_root);
        Self::with_memory_config(aibe_root, memory_config)
    }

    fn spaces_root(&self) -> PathBuf {
        self.aibe_root.join("memory").join("spaces")
    }

    fn space_dir(&self, memory_space_id: &str) -> PathBuf {
        self.spaces_root().join(memory_space_id)
    }

    fn space_events_path(&self, memory_space_id: &str) -> PathBuf {
        self.space_dir(memory_space_id).join("events.jsonl")
    }

    fn space_lock_path(&self, memory_space_id: &str) -> PathBuf {
        self.space_dir(memory_space_id).join(".lock")
    }

    fn legacy_events_path(&self, session_id: &str) -> PathBuf {
        self.aibe_root
            .join("conversations")
            .join(session_id)
            .join("memory")
            .join("events.jsonl")
    }

    fn ensure_space_layout(&self, memory_space_id: &str) -> Result<(), ContextualMemoryStoreError> {
        create_dir_0700(&self.spaces_root()).map_err(io_err)?;
        create_dir_0700(&self.space_dir(memory_space_id)).map_err(io_err)?;
        Ok(())
    }

    fn with_space_lock<T>(
        &self,
        memory_space_id: &str,
        f: impl FnOnce() -> Result<T, ContextualMemoryStoreError>,
    ) -> Result<T, ContextualMemoryStoreError> {
        self.ensure_space_layout(memory_space_id)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(self.space_lock_path(memory_space_id))
            .map_err(io_err)?;
        let guard = SpaceLock::acquire(lock)?;
        let result = f();
        drop(guard);
        result
    }

    fn ensure_lazy_copy(
        &self,
        memory_space_id: &str,
        session_id: &str,
    ) -> Result<(), ContextualMemoryStoreError> {
        // legacy space 自身は read-through（query では copy しない）
        if memory_space_id == legacy_session_memory_space_id(session_id) {
            return Ok(());
        }
        self.copy_legacy_into_space(memory_space_id, session_id)
    }

    /// legacy events を new layout へ copy する（space 未作成かつ legacy 存在時のみ）。
    /// 元の legacy store は変更しない。
    fn copy_legacy_into_space(
        &self,
        memory_space_id: &str,
        session_id: &str,
    ) -> Result<(), ContextualMemoryStoreError> {
        let space_path = self.space_events_path(memory_space_id);
        if space_path.exists() {
            return Ok(());
        }
        let legacy_path = self.legacy_events_path(session_id);
        if !legacy_path.exists() {
            return Ok(());
        }
        self.ensure_space_layout(memory_space_id)?;
        fs::copy(&legacy_path, &space_path).map_err(io_err)?;
        set_permissions_0600(&space_path).ok();
        Ok(())
    }

    fn load_entries(
        &self,
        memory_space_id: &str,
        session_id: &str,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        if !is_valid_session_id(session_id) {
            return Err(ContextualMemoryStoreError::Validation(
                MemoryValidationError::InvalidSessionId(session_id.to_string()),
            ));
        }
        self.ensure_lazy_copy(memory_space_id, session_id)?;
        let space_path = self.space_events_path(memory_space_id);
        if space_path.exists() {
            return self.load_from_path(&space_path, memory_space_id, session_id);
        }
        if memory_space_id == legacy_session_memory_space_id(session_id) {
            let legacy_path = self.legacy_events_path(session_id);
            if legacy_path.exists() {
                return self.load_from_path(&legacy_path, memory_space_id, session_id);
            }
        }
        Ok(vec![])
    }

    fn load_from_path(
        &self,
        path: &Path,
        default_space_id: &str,
        fallback_session: &str,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        let file = File::open(path).map_err(io_err)?;
        set_permissions_0600(path).ok();
        let reader = BufReader::new(file);
        let mut map: HashMap<String, MemoryEntry> = HashMap::new();
        for line in reader.lines() {
            let line = line.map_err(io_err)?;
            if line.trim().is_empty() {
                continue;
            }
            let event: StoredEventRaw =
                serde_json::from_str(&line).map_err(|e| io_err(e.to_string()))?;
            apply_event_raw(&mut map, event, default_space_id, fallback_session);
        }
        Ok(map.into_values().collect())
    }

    fn append_event(
        &self,
        memory_space_id: &str,
        event: StoredEvent,
    ) -> Result<(), ContextualMemoryStoreError> {
        let path = self.space_events_path(memory_space_id);
        self.ensure_space_layout(memory_space_id)?;
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

    /// テスト用: space 配下の entries を直接読む。
    #[cfg(test)]
    pub fn load_entries_for_test(
        &self,
        memory_space_id: &str,
        session_id: &str,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        self.load_entries(memory_space_id, session_id)
    }
}

impl ContextualMemoryStore for FilesystemContextualMemoryStore {
    fn apply(
        &self,
        ctx: &MemoryStoreContext<'_>,
        operation: &MemoryOperationDto,
        now_ms: u64,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        if !is_valid_session_id(ctx.session_id) {
            return Err(ContextualMemoryStoreError::Validation(
                MemoryValidationError::InvalidSessionId(ctx.session_id.to_string()),
            ));
        }
        let project_key = Self::project_key_from_cwd(ctx.cwd)?;
        let memory_space_id = ctx.memory_space_id.as_str();
        let session_id = ctx.session_id;
        let registry = self
            .registry_loader
            .load_strict(memory_space_id)
            .map_err(ContextualMemoryStoreError::Registry)?;
        self.with_space_lock(memory_space_id, || {
            // 書き込み時は legacy space でも new layout へ seed し、
            // append 後に legacy data が見えなくなる分裂を防ぐ。
            self.copy_legacy_into_space(memory_space_id, session_id)?;
            match operation {
                MemoryOperationDto::Add(add) => {
                    let kind = &add.kind;
                    validate_kind(kind)?;
                    validate_text(&add.text)?;
                    let scope_dto = add.scope.ok_or_else(|| {
                        ContextualMemoryStoreError::Validation(
                            MemoryValidationError::UnregisteredKindMissingFields {
                                kind: kind.clone(),
                            },
                        )
                    })?;
                    let inject_dto = add.inject.ok_or_else(|| {
                        ContextualMemoryStoreError::Validation(
                            MemoryValidationError::UnregisteredKindMissingFields {
                                kind: kind.clone(),
                            },
                        )
                    })?;
                    let status_dto = add.status.ok_or_else(|| {
                        ContextualMemoryStoreError::Validation(
                            MemoryValidationError::UnregisteredKindMissingFields {
                                kind: kind.clone(),
                            },
                        )
                    })?;
                    let make_active = add.make_active.unwrap_or(false);
                    if registry.is_registered(kind) {
                        validate_standard_kind_operation(
                            &registry, kind, scope_dto, inject_dto, status_dto,
                        )?;
                    }
                    let scope = MemoryScope::try_from(scope_dto).map_err(|_| {
                        ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                            "scope".into(),
                        ))
                    })?;
                    if scope == MemoryScope::Project && project_key.is_none() {
                        return Err(ContextualMemoryStoreError::Validation(
                            MemoryValidationError::MissingCwdForProjectScope,
                        ));
                    }
                    let inject = MemoryInjectPolicy::try_from(inject_dto).map_err(|_| {
                        ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                            "inject".into(),
                        ))
                    })?;
                    let client_status = MemoryStatus::try_from(status_dto).map_err(|_| {
                        ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                            "status".into(),
                        ))
                    })?;
                    let final_status = if make_active {
                        MemoryStatus::Active
                    } else {
                        client_status
                    };

                    let mut affected = Vec::new();
                    if make_active {
                        let mut entries = self.load_entries(memory_space_id, session_id)?;
                        for entry in entries.iter_mut() {
                            if entry.kind == *kind
                                && entry.scope == scope
                                && entry.status == MemoryStatus::Active
                                && scope_matches_project_key(
                                    scope,
                                    &entry.project_key,
                                    &project_key,
                                )
                            {
                                entry.status = MemoryStatus::Inactive;
                                entry.updated_at_ms = now_ms;
                                entry.last_session_id = session_id.to_string();
                                entry.version += 1;
                                self.append_event(
                                    memory_space_id,
                                    StoredEvent::StatusChanged {
                                        id: entry.id.clone(),
                                        status: MemoryStatus::Inactive,
                                        updated_at_ms: now_ms,
                                        last_session_id: entry.last_session_id.clone(),
                                        version: entry.version,
                                    },
                                )?;
                                affected.push(entry.clone());
                            }
                        }
                    }

                    let entries = self.load_entries(memory_space_id, session_id)?;
                    let id = next_id(&entries, memory_space_id);
                    let version = 1;
                    let new_entry = MemoryEntry {
                        id: id.clone(),
                        memory_space_id: memory_space_id.to_string(),
                        created_session_id: session_id.to_string(),
                        last_session_id: session_id.to_string(),
                        kind: kind.clone(),
                        scope,
                        inject,
                        status: final_status,
                        text: add.text.clone(),
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
                        memory_space_id,
                        StoredEvent::Added {
                            entry: new_entry.clone(),
                        },
                    )?;
                    affected.push(new_entry);
                    Ok(affected)
                }
                MemoryOperationDto::ClearKind(clear) => {
                    let kind = &clear.kind;
                    validate_kind(kind)?;
                    let scope = MemoryScope::try_from(clear.scope).map_err(|_| {
                        ContextualMemoryStoreError::Validation(MemoryValidationError::InvalidKind(
                            "scope".into(),
                        ))
                    })?;
                    if scope == MemoryScope::Project && project_key.is_none() {
                        return Err(ContextualMemoryStoreError::Validation(
                            MemoryValidationError::MissingCwdForProjectScope,
                        ));
                    }
                    let (from_status, to_status) = registry.clear_transition(kind);
                    let mut entries = self.load_entries(memory_space_id, session_id)?;
                    let mut affected = Vec::new();
                    for entry in entries.iter_mut() {
                        if entry.kind == *kind
                            && entry.scope == scope
                            && entry.status == from_status
                            && scope_matches_project_key(scope, &entry.project_key, &project_key)
                        {
                            entry.status = to_status;
                            entry.updated_at_ms = now_ms;
                            entry.last_session_id = session_id.to_string();
                            entry.version += 1;
                            self.append_event(
                                memory_space_id,
                                StoredEvent::StatusChanged {
                                    id: entry.id.clone(),
                                    status: to_status,
                                    updated_at_ms: now_ms,
                                    last_session_id: entry.last_session_id.clone(),
                                    version: entry.version,
                                },
                            )?;
                            affected.push(entry.clone());
                        }
                    }
                    Ok(affected)
                }
                MemoryOperationDto::Archive(archive) => {
                    let mut entries = self.load_entries(memory_space_id, session_id)?;
                    let entry = entries
                        .iter_mut()
                        .find(|e| e.id == archive.id)
                        .ok_or_else(|| ContextualMemoryStoreError::NotFound(archive.id.clone()))?;
                    if let Some(expected) = archive.expected_version {
                        if entry.version != expected {
                            return Err(ContextualMemoryStoreError::Validation(
                                MemoryValidationError::VersionConflict,
                            ));
                        }
                    }
                    entry.status = MemoryStatus::Archived;
                    entry.updated_at_ms = now_ms;
                    entry.last_session_id = session_id.to_string();
                    entry.version += 1;
                    self.append_event(
                        memory_space_id,
                        StoredEvent::StatusChanged {
                            id: entry.id.clone(),
                            status: MemoryStatus::Archived,
                            updated_at_ms: now_ms,
                            last_session_id: entry.last_session_id.clone(),
                            version: entry.version,
                        },
                    )?;
                    Ok(vec![entry.clone()])
                }
            }
        })
    }

    fn query(
        &self,
        ctx: &MemoryStoreContext<'_>,
        query: &MemoryQueryDto,
    ) -> Result<Vec<MemoryEntry>, ContextualMemoryStoreError> {
        if !is_valid_session_id(ctx.session_id) {
            return Err(ContextualMemoryStoreError::Validation(
                MemoryValidationError::InvalidSessionId(ctx.session_id.to_string()),
            ));
        }
        let project_key = Self::project_key_from_cwd(ctx.cwd)?;
        self.with_space_lock(ctx.memory_space_id.as_str(), || {
            let mut entries = self.load_entries(ctx.memory_space_id.as_str(), ctx.session_id)?;
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
        ctx: &MemoryStoreContext<'_>,
        user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        self.resolve_for_prompt_inner(ctx, user_query, budget_bytes, false)
    }

    fn resolve_for_prompt_explicit(
        &self,
        ctx: &MemoryStoreContext<'_>,
        user_query: &str,
        budget_bytes: usize,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        self.resolve_for_prompt_inner(ctx, user_query, budget_bytes, true)
    }
}

impl FilesystemContextualMemoryStore {
    fn resolve_for_prompt_inner(
        &self,
        ctx: &MemoryStoreContext<'_>,
        user_query: &str,
        budget_bytes: usize,
        strict_registry: bool,
    ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
        if ctx.session_id.is_empty() {
            return Ok(MemoryBlock {
                content: String::new(),
            });
        }
        let project_key = Self::project_key_from_cwd(ctx.cwd)?;
        let registry = if strict_registry {
            self.registry_loader
                .load_strict(ctx.memory_space_id.as_str())
                .map_err(ContextualMemoryStoreError::Registry)?
        } else {
            self.registry_loader
                .load_best_effort(ctx.memory_space_id.as_str())
        };
        let entries = self.with_space_lock(ctx.memory_space_id.as_str(), || {
            self.load_entries(ctx.memory_space_id.as_str(), ctx.session_id)
        })?;
        Ok(resolve_entries_for_prompt(
            &entries,
            &registry,
            project_key.as_deref(),
            ctx.session_id,
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

fn next_id(entries: &[MemoryEntry], memory_space_id: &str) -> String {
    let seq = entries.len() + 1;
    format!("mem_{memory_space_id}_{seq}")
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum StoredEvent {
    Added {
        entry: MemoryEntry,
    },
    StatusChanged {
        id: String,
        status: MemoryStatus,
        updated_at_ms: u64,
        last_session_id: String,
        version: u64,
    },
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum StoredEventRaw {
    Added {
        entry: StoredMemoryEntry,
    },
    StatusChanged {
        id: String,
        status: MemoryStatus,
        updated_at_ms: u64,
        #[serde(default)]
        last_session_id: String,
        version: u64,
    },
}

#[derive(Debug, Clone, serde::Deserialize)]
struct StoredMemoryEntry {
    id: String,
    #[serde(default)]
    memory_space_id: Option<String>,
    #[serde(default)]
    created_session_id: Option<String>,
    #[serde(default)]
    last_session_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    kind: String,
    scope: MemoryScope,
    inject: MemoryInjectPolicy,
    status: MemoryStatus,
    text: String,
    #[serde(default)]
    project_key: Option<String>,
    created_at_ms: u64,
    updated_at_ms: u64,
    version: u64,
}

impl StoredMemoryEntry {
    fn into_entry(self, default_space_id: &str, fallback_session: &str) -> MemoryEntry {
        let legacy_sid = self
            .session_id
            .or(self.created_session_id.clone())
            .unwrap_or_else(|| fallback_session.to_string());
        MemoryEntry {
            id: self.id,
            memory_space_id: self
                .memory_space_id
                .unwrap_or_else(|| default_space_id.to_string()),
            created_session_id: self
                .created_session_id
                .unwrap_or_else(|| legacy_sid.clone()),
            last_session_id: self.last_session_id.unwrap_or(legacy_sid),
            kind: self.kind,
            scope: self.scope,
            inject: self.inject,
            status: self.status,
            text: self.text,
            project_key: self.project_key,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            version: self.version,
        }
    }
}

fn apply_event_raw(
    map: &mut HashMap<String, MemoryEntry>,
    event: StoredEventRaw,
    default_space_id: &str,
    fallback_session: &str,
) {
    match event {
        StoredEventRaw::Added { entry } => {
            let entry = entry.into_entry(default_space_id, fallback_session);
            map.insert(entry.id.clone(), entry);
        }
        StoredEventRaw::StatusChanged {
            id,
            status,
            updated_at_ms,
            last_session_id,
            version,
        } => {
            if let Some(entry) = map.get_mut(&id) {
                entry.status = status;
                entry.updated_at_ms = updated_at_ms;
                if !last_session_id.is_empty() {
                    entry.last_session_id = last_session_id;
                }
                entry.version = version;
            }
        }
    }
}

fn io_err(e: impl ToString) -> ContextualMemoryStoreError {
    ContextualMemoryStoreError::Io(e.to_string())
}

struct SpaceLock(File);

impl SpaceLock {
    fn acquire(file: File) -> Result<Self, ContextualMemoryStoreError> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if rc != 0 {
                return Err(io_err("failed to acquire memory space lock"));
            }
        }
        Ok(Self(file))
    }
}

impl Drop for SpaceLock {
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
    use crate::domain::test_support::{STANDARD_KIND_GOAL, STANDARD_KIND_NOW};
    use aibe_protocol::{
        MemoryInjectPolicyDto, MemoryOperationAdd, MemoryOperationDto, MemoryScopeDto,
        MemoryStatusDto,
    };
    use tempfile::TempDir;

    fn store() -> (FilesystemContextualMemoryStore, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        (
            FilesystemContextualMemoryStore::new(dir.path().to_path_buf()),
            dir,
        )
    }

    fn ctx<'a>(
        session_id: &'a str,
        memory_space_id: &str,
        cwd: Option<&'a Path>,
    ) -> MemoryStoreContext<'a> {
        MemoryStoreContext {
            session_id,
            memory_space_id: memory_space_id.to_string(),
            cwd,
        }
    }

    fn goal_add(text: &str) -> MemoryOperationDto {
        MemoryOperationDto::Add(MemoryOperationAdd {
            kind: STANDARD_KIND_GOAL.into(),
            scope: Some(MemoryScopeDto::Project),
            inject: Some(MemoryInjectPolicyDto::Pinned),
            status: Some(MemoryStatusDto::Active),
            text: text.into(),
            make_active: Some(true),
        })
    }

    #[test]
    fn goal_add_creates_active_goal() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        let c = ctx("sess", "ctx_a", Some(&cwd));
        let entries = store.apply(&c, &goal_add("first"), 1).expect("apply");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, MemoryStatus::Active);
        assert_eq!(entries[0].memory_space_id, "ctx_a");
    }

    #[test]
    fn goal_add_twice_inactivates_old() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        let c = ctx("sess", "ctx_a", Some(&cwd));
        store.apply(&c, &goal_add("first"), 1).expect("apply");
        let second = store.apply(&c, &goal_add("second"), 2).expect("apply");
        assert_eq!(second.last().expect("new").text, "second");
        let all = store.load_entries_for_test("ctx_a", "sess").expect("load");
        let active: Vec<_> = all
            .iter()
            .filter(|e| e.kind == STANDARD_KIND_GOAL && e.status == MemoryStatus::Active)
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].text, "second");
    }

    fn now_add(text: &str) -> MemoryOperationDto {
        MemoryOperationDto::Add(MemoryOperationAdd {
            kind: STANDARD_KIND_NOW.into(),
            scope: Some(MemoryScopeDto::Session),
            inject: Some(MemoryInjectPolicyDto::Pinned),
            status: Some(MemoryStatusDto::Active),
            text: text.into(),
            make_active: Some(true),
        })
    }

    #[test]
    fn now_add_twice_inactivates_old() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        let c = ctx("sess", "ctx_a", Some(&cwd));
        store.apply(&c, &now_add("first"), 1).expect("apply");
        let second = store.apply(&c, &now_add("second"), 2).expect("apply");
        assert_eq!(second.last().expect("new").text, "second");
    }

    #[test]
    fn different_sessions_share_memory_space() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        let c1 = ctx("sess_001", "ctx_a", Some(&cwd));
        store
            .apply(&c1, &goal_add("shared goal"), 1)
            .expect("apply");
        let c2 = ctx("sess_002", "ctx_a", Some(&cwd));
        let entries = store
            .query(
                &c2,
                &MemoryQueryDto {
                    kind: Some(STANDARD_KIND_GOAL.into()),
                    scope: Some(MemoryScopeDto::Project),
                    status: Some(MemoryStatusDto::Active),
                    active_only: true,
                    include_archived: false,
                    limit: None,
                    include_prompt_block: false,
                    user_query: None,
                },
            )
            .expect("query");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "shared goal");
    }

    /// 0034 形式（`session_id` のみ、`memory_space_id` なし）の legacy events を書く。
    fn write_legacy_events(aibe_root: &Path, session_id: &str, text: &str) -> PathBuf {
        let dir = aibe_root
            .join("conversations")
            .join(session_id)
            .join("memory");
        fs::create_dir_all(&dir).expect("legacy dir");
        let path = dir.join("events.jsonl");
        let line = format!(
            r#"{{"event":"added","entry":{{"id":"mem_{session_id}_1","session_id":"{session_id}","kind":"goal","scope":"global","inject":"pinned","status":"active","text":"{text}","created_at_ms":1,"updated_at_ms":1,"version":1}}}}"#
        );
        fs::write(&path, format!("{line}\n")).expect("write legacy events");
        path
    }

    #[test]
    fn lazy_copy_seeds_named_space_and_keeps_legacy_store_intact() {
        let (store, dir) = store();
        let legacy_path = write_legacy_events(dir.path(), "sess_001", "legacy goal");
        let original = fs::read_to_string(&legacy_path).expect("read legacy");

        let cwd = std::env::current_dir().expect("cwd");
        let c1 = ctx("sess_001", "ctx_a", Some(&cwd));
        let entries = store
            .query(
                &c1,
                &MemoryQueryDto {
                    kind: Some(STANDARD_KIND_GOAL.into()),
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
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "legacy goal");

        // new layout に copy され、元の legacy store は無傷
        assert!(dir
            .path()
            .join("memory/spaces/ctx_a/events.jsonl")
            .is_file());
        assert_eq!(
            fs::read_to_string(&legacy_path).expect("re-read legacy"),
            original
        );

        // 別 session から同じ named space を見ても同じ state
        let c2 = ctx("sess_002", "ctx_a", Some(&cwd));
        let entries = store
            .query(
                &c2,
                &MemoryQueryDto {
                    kind: Some(STANDARD_KIND_GOAL.into()),
                    scope: None,
                    status: None,
                    active_only: false,
                    include_archived: false,
                    limit: None,
                    include_prompt_block: false,
                    user_query: None,
                },
            )
            .expect("query from sess_002");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "legacy goal");
    }

    #[test]
    fn legacy_session_space_reads_through_without_copy() {
        let (store, dir) = store();
        let legacy_path = write_legacy_events(dir.path(), "sess_001", "legacy goal");
        let original = fs::read_to_string(&legacy_path).expect("read legacy");

        let cwd = std::env::current_dir().expect("cwd");
        let c = ctx("sess_001", "legacy_session_sess_001", Some(&cwd));
        let entries = store
            .query(
                &c,
                &MemoryQueryDto {
                    kind: Some(STANDARD_KIND_GOAL.into()),
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
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "legacy goal");

        // read-through のみで copy しない（legacy 自身が正本）
        assert!(!dir
            .path()
            .join("memory/spaces/legacy_session_sess_001/events.jsonl")
            .exists());
        assert_eq!(
            fs::read_to_string(&legacy_path).expect("re-read legacy"),
            original
        );

        // 新規書き込みは legacy data ごと new layout へ seed され、両方見える
        store.apply(&c, &goal_add("new goal"), 2).expect("apply");
        assert!(dir
            .path()
            .join("memory/spaces/legacy_session_sess_001/events.jsonl")
            .is_file());
        assert_eq!(
            fs::read_to_string(&legacy_path).expect("re-read legacy after write"),
            original
        );
        let entries = store
            .load_entries_for_test("legacy_session_sess_001", "sess_001")
            .expect("load after write");
        let texts: Vec<_> = entries.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"legacy goal"));
        assert!(texts.contains(&"new goal"));
    }

    #[test]
    fn different_memory_spaces_are_isolated() {
        let (store, _dir) = store();
        let cwd = std::env::current_dir().expect("cwd");
        let c1 = ctx("sess_001", "ctx_a", Some(&cwd));
        store.apply(&c1, &goal_add("ctx a goal"), 1).expect("apply");
        let c3 = ctx("sess_003", "ctx_b", Some(&cwd));
        let entries = store
            .query(
                &c3,
                &MemoryQueryDto {
                    kind: Some(STANDARD_KIND_GOAL.into()),
                    scope: Some(MemoryScopeDto::Project),
                    status: Some(MemoryStatusDto::Active),
                    active_only: true,
                    include_archived: false,
                    limit: None,
                    include_prompt_block: false,
                    user_query: None,
                },
            )
            .expect("query");
        assert!(entries.is_empty());
    }
}
