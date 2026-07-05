//! filesystem handoff store（`~/.local/share/aibe/handoffs/`）。

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::domain::{
    finalize_running_tools, hash_handoff_token, mark_tool_completed, validate_handoff_id,
    CollaborativeAuditKind, CommandCandidate, Handoff, HandoffCheckpoint, HandoffLease,
    HandoffShellSession, HandoffState, RecoverableToolStatus,
};
use crate::ports::outbound::{
    CheckpointRepository, CommandCandidateStore, HandoffRepository, HandoffShellSessionStore,
    HandoffStoreError, LeaseAcquireRequest, LeaseHeartbeatRequest, LeaseRepository,
    ShellSessionIssueRequest,
};

#[derive(Debug, Clone)]
pub struct FilesystemHandoffStore {
    root: PathBuf,
}

impl FilesystemHandoffStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_root() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".local/share/aibe/handoffs")
    }

    /// status 用のローカル列挙。壊れた個別 entry は他の handoff の表示を妨げない。
    fn read_handoffs(&self) -> Result<Vec<Handoff>, HandoffStoreError> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&self.root).map_err(read_err)?;
        let mut handoffs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(read_err)?;
            if !entry.file_type().map_err(read_err)?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if validate_handoff_id(&id).is_err() {
                continue;
            }
            if let Ok(handoff) = self.load_handoff(&id) {
                handoffs.push(handoff);
            }
        }
        handoffs.sort_by_key(|handoff| std::cmp::Reverse(handoff.updated_at_ms));
        Ok(handoffs)
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.jsonl")
    }

    fn index_lock_path(&self) -> PathBuf {
        self.root.join(".index.lock")
    }

    fn lock_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join(".lock"))
    }

    fn handoff_dir(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        validate_handoff_id(handoff_id).map_err(|_| HandoffStoreError::InvalidHandoffId)?;
        Ok(self.root.join(handoff_id))
    }

    fn handoff_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join("handoff.json"))
    }

    fn lease_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join("lease.json"))
    }

    fn side_run_lock_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join("side-run-lock.json"))
    }

    fn checkpoint_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join("checkpoint.json"))
    }

    fn shell_sessions_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join("shell_sessions.jsonl"))
    }

    fn candidates_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join("candidates.jsonl"))
    }

    fn events_path(&self, handoff_id: &str) -> Result<PathBuf, HandoffStoreError> {
        Ok(self.handoff_dir(handoff_id)?.join("events.jsonl"))
    }

    fn ensure_handoff_layout(&self, handoff_id: &str) -> Result<(), HandoffStoreError> {
        create_dir_0700(&self.root).map_err(write_err)?;
        create_dir_0700(&self.handoff_dir(handoff_id)?).map_err(write_err)?;
        Ok(())
    }

    fn with_handoff_lock<T>(
        &self,
        handoff_id: &str,
        f: impl FnOnce() -> Result<T, HandoffStoreError>,
    ) -> Result<T, HandoffStoreError> {
        self.ensure_handoff_layout(handoff_id)?;
        let lock_path = self.lock_path(handoff_id)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(write_err)?;
        set_permissions_0600(&lock_path).map_err(write_err)?;
        let guard = HandoffLock::acquire(lock)?;
        let result = f();
        drop(guard);
        result
    }

    fn with_index_lock<T>(
        &self,
        f: impl FnOnce() -> Result<T, HandoffStoreError>,
    ) -> Result<T, HandoffStoreError> {
        create_dir_0700(&self.root).map_err(write_err)?;
        let lock_path = self.index_lock_path();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(write_err)?;
        set_permissions_0600(&lock_path).map_err(write_err)?;
        let guard = HandoffLock::acquire(lock)?;
        let result = f();
        drop(guard);
        result
    }

    fn write_json_atomic<T: serde::Serialize>(
        &self,
        path: &Path,
        value: &T,
    ) -> Result<(), HandoffStoreError> {
        if let Some(parent) = path.parent() {
            create_dir_0700(parent).map_err(write_err)?;
        }
        let json = serde_json::to_string_pretty(value).map_err(write_err)?;
        let temp = path.with_extension("json.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&temp)
                .map_err(write_err)?;
            file.write_all(json.as_bytes()).map_err(write_err)?;
            file.sync_all().map_err(write_err)?;
        }
        set_permissions_0600(&temp).map_err(write_err)?;
        fs::rename(&temp, path).map_err(write_err)?;
        set_permissions_0600(path).map_err(write_err)?;
        Ok(())
    }

    fn read_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &Path,
    ) -> Result<T, HandoffStoreError> {
        if !path.is_file() {
            return Err(HandoffStoreError::NotFound(path.display().to_string()));
        }
        let raw = fs::read_to_string(path).map_err(read_err)?;
        serde_json::from_str(raw.trim()).map_err(read_err)
    }

    fn append_jsonl<T: serde::Serialize>(
        &self,
        path: &Path,
        value: &T,
    ) -> Result<(), HandoffStoreError> {
        if let Some(parent) = path.parent() {
            create_dir_0700(parent).map_err(write_err)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(write_err)?;
        let line = serde_json::to_string(value).map_err(write_err)?;
        writeln!(file, "{line}").map_err(write_err)?;
        file.sync_all().map_err(write_err)?;
        set_permissions_0600(path).map_err(write_err)?;
        Ok(())
    }

    fn read_jsonl<T: serde::de::DeserializeOwned>(
        &self,
        path: &Path,
    ) -> Result<Vec<T>, HandoffStoreError> {
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let file = File::open(path).map_err(read_err)?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(read_err)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push(serde_json::from_str(trimmed).map_err(read_err)?);
        }
        Ok(out)
    }

    fn append_audit_event(
        &self,
        handoff_id: &str,
        kind: CollaborativeAuditKind,
    ) -> Result<(), HandoffStoreError> {
        #[derive(serde::Serialize)]
        struct AuditEvent {
            event: CollaborativeAuditKind,
            handoff_id: String,
            at_ms: u64,
        }
        let at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.append_jsonl(
            &self.events_path(handoff_id)?,
            &AuditEvent {
                event: kind,
                handoff_id: handoff_id.to_string(),
                at_ms,
            },
        )
    }

    pub fn record_audit(
        &self,
        handoff_id: &str,
        kind: CollaborativeAuditKind,
    ) -> Result<(), HandoffStoreError> {
        self.append_audit_event(handoff_id, kind)
    }

    fn update_index(&self, handoff: &Handoff) -> Result<(), HandoffStoreError> {
        #[derive(serde::Serialize)]
        struct IndexEntry<'a> {
            handoff_id: &'a str,
            state: HandoffState,
            updated_at_ms: u64,
        }
        self.append_jsonl(
            &self.index_path(),
            &IndexEntry {
                handoff_id: &handoff.id,
                state: handoff.state,
                updated_at_ms: handoff.updated_at_ms,
            },
        )
    }
}

impl HandoffRepository for FilesystemHandoffStore {
    fn save_handoff(&self, handoff: &Handoff) -> Result<(), HandoffStoreError> {
        validate_handoff_id(&handoff.id).map_err(|_| HandoffStoreError::InvalidHandoffId)?;
        self.with_handoff_lock(&handoff.id, || {
            self.write_json_atomic(&self.handoff_path(&handoff.id)?, handoff)
        })?;
        self.with_index_lock(|| self.update_index(handoff))
    }

    fn load_handoff(&self, handoff_id: &str) -> Result<Handoff, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            self.read_json(&self.handoff_path(handoff_id)?)
        })
    }

    fn list_handoffs(&self) -> Result<Vec<Handoff>, HandoffStoreError> {
        self.read_handoffs()
    }
}

impl LeaseRepository for FilesystemHandoffStore {
    fn try_acquire_lease(
        &self,
        handoff_id: &str,
        request: &LeaseAcquireRequest,
    ) -> Result<HandoffLease, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.lease_path(handoff_id)?;
            if path.is_file() {
                let existing: HandoffLease = self.read_json(&path)?;
                if existing.lease_expires_at_ms > request.now_ms
                    && !same_lease_owner(&existing, request)
                {
                    return Err(HandoffStoreError::LeaseConflict);
                }
            }
            let lease = HandoffLease {
                handoff_id: handoff_id.to_string(),
                owner_client_id: request.owner_client_id.clone(),
                owner_process_id: request.owner_process_id,
                owner_tty: request.owner_tty.clone(),
                owner_host: request.owner_host.clone(),
                owner_uid: request.owner_uid,
                lease_acquired_at_ms: request.now_ms,
                lease_expires_at_ms: request.now_ms.saturating_add(request.lease_timeout_ms),
                last_heartbeat_at_ms: request.now_ms,
            };
            self.write_json_atomic(&path, &lease)?;
            Ok(lease)
        })
    }

    fn load_lease(&self, handoff_id: &str) -> Result<Option<HandoffLease>, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.lease_path(handoff_id)?;
            if !path.is_file() {
                return Ok(None);
            }
            Ok(Some(self.read_json(&path)?))
        })
    }

    fn heartbeat_lease(
        &self,
        handoff_id: &str,
        request: &LeaseHeartbeatRequest,
    ) -> Result<HandoffLease, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.lease_path(handoff_id)?;
            let mut lease: HandoffLease = self.read_json(&path)?;
            if lease.owner_client_id != request.owner_client_id
                || lease.owner_process_id != request.owner_process_id
                || lease.lease_expires_at_ms <= request.now_ms
            {
                return Err(HandoffStoreError::LeaseConflict);
            }
            lease.last_heartbeat_at_ms = request.now_ms;
            lease.lease_expires_at_ms = request.now_ms.saturating_add(request.lease_timeout_ms);
            self.write_json_atomic(&path, &lease)?;
            Ok(lease)
        })
    }

    fn release_lease(&self, handoff_id: &str) -> Result<(), HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.lease_path(handoff_id)?;
            if path.exists() {
                fs::remove_file(path).map_err(write_err)?;
            }
            Ok(())
        })
    }
}

impl crate::ports::outbound::SideRunLockRepository for FilesystemHandoffStore {
    fn try_acquire_side_run_lock(
        &self,
        handoff_id: &str,
        request: &LeaseAcquireRequest,
    ) -> Result<HandoffLease, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.side_run_lock_path(handoff_id)?;
            if path.is_file() {
                let existing: HandoffLease = self.read_json(&path)?;
                if existing.lease_expires_at_ms > request.now_ms {
                    return Err(HandoffStoreError::LeaseConflict);
                }
            }
            let lock = HandoffLease {
                handoff_id: handoff_id.to_string(),
                owner_client_id: request.owner_client_id.clone(),
                owner_process_id: request.owner_process_id,
                owner_tty: request.owner_tty.clone(),
                owner_host: request.owner_host.clone(),
                owner_uid: request.owner_uid,
                lease_acquired_at_ms: request.now_ms,
                lease_expires_at_ms: request.now_ms.saturating_add(request.lease_timeout_ms),
                last_heartbeat_at_ms: request.now_ms,
            };
            self.write_json_atomic(&path, &lock)?;
            Ok(lock)
        })
    }

    fn release_side_run_lock(&self, handoff_id: &str) -> Result<(), HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.side_run_lock_path(handoff_id)?;
            if path.exists() {
                fs::remove_file(path).map_err(write_err)?;
            }
            Ok(())
        })
    }

    fn load_side_run_lock(
        &self,
        handoff_id: &str,
    ) -> Result<Option<HandoffLease>, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let path = self.side_run_lock_path(handoff_id)?;
            if !path.is_file() {
                return Ok(None);
            }
            Ok(Some(self.read_json(&path)?))
        })
    }

    fn recover_stale_side_agent_run(
        &self,
        handoff_id: &str,
        owner_is_alive: &dyn Fn(u32) -> bool,
        now_ms: u64,
        update: &mut dyn FnMut(
            &mut Handoff,
            &mut HandoffCheckpoint,
        ) -> Result<(), HandoffStoreError>,
    ) -> Result<bool, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let mut handoff: Handoff = self.read_json(&self.handoff_path(handoff_id)?)?;
            if handoff.state != HandoffState::SideAgentRunning {
                return Ok(false);
            }
            let side_path = self.side_run_lock_path(handoff_id)?;
            if side_path.is_file() {
                let lock: HandoffLease = self.read_json(&side_path)?;
                if owner_is_alive(lock.owner_process_id) {
                    return Ok(false);
                }
                fs::remove_file(&side_path).map_err(write_err)?;
            }
            let mut checkpoint: HandoffCheckpoint =
                self.read_json(&self.checkpoint_path(handoff_id)?)?;
            update(&mut handoff, &mut checkpoint)?;
            handoff.updated_at_ms = now_ms;
            self.write_json_atomic(&self.checkpoint_path(handoff_id)?, &checkpoint)?;
            self.write_json_atomic(&self.handoff_path(handoff_id)?, &handoff)?;
            Ok(true)
        })
    }

    fn finish_side_run_atomically(
        &self,
        handoff_id: &str,
        now_ms: u64,
        candidates: &[CommandCandidate],
        resume_tool_call_id: Option<&str>,
        update: &mut dyn FnMut(
            &mut Handoff,
            &mut HandoffCheckpoint,
        ) -> Result<(), HandoffStoreError>,
    ) -> Result<(), HandoffStoreError> {
        fn apply_tool_finalization(checkpoint: &mut HandoffCheckpoint, tool_call_id: Option<&str>) {
            if let Some(call_id) = tool_call_id {
                mark_tool_completed(checkpoint, call_id, "aish.request_human_action");
                finalize_running_tools(checkpoint, RecoverableToolStatus::Unknown, Some(call_id));
            }
        }

        self.with_handoff_lock(handoff_id, || {
            let mut handoff: Handoff = self.read_json(&self.handoff_path(handoff_id)?)?;
            let mut checkpoint: HandoffCheckpoint =
                self.read_json(&self.checkpoint_path(handoff_id)?)?;
            let side_path = self.side_run_lock_path(handoff_id)?;
            let lock_exists = side_path.exists();

            if handoff.state == HandoffState::SideAgentRunning {
                update(&mut handoff, &mut checkpoint)?;
                apply_tool_finalization(&mut checkpoint, resume_tool_call_id);
                handoff.updated_at_ms = now_ms;
                self.write_json_atomic(&self.checkpoint_path(handoff_id)?, &checkpoint)?;
                self.write_json_atomic(&self.handoff_path(handoff_id)?, &handoff)?;
            } else if lock_exists && checkpoint.control_state != handoff.state {
                if checkpoint.control_state == HandoffState::SideAgentRunning
                    && matches!(
                        handoff.state,
                        HandoffState::SideAgentWaitingForHuman | HandoffState::HumanActive
                    )
                {
                    checkpoint.control_state = handoff.state;
                    checkpoint.conversation_summary = handoff.conversation_summary.clone();
                    apply_tool_finalization(&mut checkpoint, resume_tool_call_id);
                    for candidate in candidates {
                        if candidate.target_handoff_id != handoff_id {
                            return Err(HandoffStoreError::InvalidHandoffId);
                        }
                        if checkpoint
                            .command_candidates
                            .iter()
                            .any(|existing| existing.id == candidate.id)
                        {
                            continue;
                        }
                        checkpoint.command_candidates.push(candidate.clone());
                    }
                    self.write_json_atomic(&self.checkpoint_path(handoff_id)?, &checkpoint)?;
                } else {
                    return Err(HandoffStoreError::Write(
                        "handoff/checkpoint state mismatch during side-run finish resume".into(),
                    ));
                }
            }

            if !candidates.is_empty() {
                let candidates_path = self.candidates_path(handoff_id)?;
                let mut existing_ids: std::collections::HashSet<String> = self
                    .read_jsonl(&candidates_path)?
                    .into_iter()
                    .map(|candidate: CommandCandidate| candidate.id)
                    .collect();
                for candidate in candidates {
                    if candidate.target_handoff_id != handoff_id {
                        return Err(HandoffStoreError::InvalidHandoffId);
                    }
                    if !existing_ids.insert(candidate.id.clone()) {
                        continue;
                    }
                    self.append_jsonl(&candidates_path, candidate)?;
                }
            }
            if lock_exists {
                fs::remove_file(&side_path).map_err(write_err)?;
            }
            Ok(())
        })
    }

    fn start_side_run_atomically(
        &self,
        handoff_id: &str,
        request: &LeaseAcquireRequest,
        owner_is_alive: &dyn Fn(u32) -> bool,
        update: &mut dyn FnMut(
            &mut Handoff,
            &mut HandoffCheckpoint,
        ) -> Result<(), HandoffStoreError>,
    ) -> Result<(), HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let mut handoff: Handoff = self.read_json(&self.handoff_path(handoff_id)?)?;
            let mut checkpoint: HandoffCheckpoint =
                self.read_json(&self.checkpoint_path(handoff_id)?)?;
            let side_path = self.side_run_lock_path(handoff_id)?;
            if side_path.is_file() {
                let existing: HandoffLease = self.read_json(&side_path)?;
                if existing.lease_expires_at_ms > request.now_ms
                    && owner_is_alive(existing.owner_process_id)
                {
                    return Err(HandoffStoreError::LeaseConflict);
                }
                fs::remove_file(&side_path).map_err(write_err)?;
            }
            update(&mut handoff, &mut checkpoint)?;
            let lock = HandoffLease {
                handoff_id: handoff_id.to_string(),
                owner_client_id: request.owner_client_id.clone(),
                owner_process_id: request.owner_process_id,
                owner_tty: request.owner_tty.clone(),
                owner_host: request.owner_host.clone(),
                owner_uid: request.owner_uid,
                lease_acquired_at_ms: request.now_ms,
                lease_expires_at_ms: request.now_ms.saturating_add(request.lease_timeout_ms),
                last_heartbeat_at_ms: request.now_ms,
            };
            self.write_json_atomic(&side_path, &lock)?;
            handoff.updated_at_ms = request.now_ms;
            self.write_json_atomic(&self.checkpoint_path(handoff_id)?, &checkpoint)?;
            self.write_json_atomic(&self.handoff_path(handoff_id)?, &handoff)?;
            Ok(())
        })
    }
}

impl CheckpointRepository for FilesystemHandoffStore {
    fn save_checkpoint(
        &self,
        handoff_id: &str,
        checkpoint: &HandoffCheckpoint,
    ) -> Result<(), HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            if checkpoint.handoff_id != handoff_id || checkpoint.child_goal.handoff_id != handoff_id
            {
                return Err(HandoffStoreError::InvalidHandoffId);
            }
            self.write_json_atomic(&self.checkpoint_path(handoff_id)?, checkpoint)
        })
    }

    fn load_checkpoint(&self, handoff_id: &str) -> Result<HandoffCheckpoint, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            self.read_json(&self.checkpoint_path(handoff_id)?)
        })
    }
}

impl HandoffShellSessionStore for FilesystemHandoffStore {
    fn append_shell_session(
        &self,
        handoff_id: &str,
        request: &ShellSessionIssueRequest,
    ) -> Result<HandoffShellSession, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            let sessions: Vec<HandoffShellSession> =
                self.read_jsonl(&self.shell_sessions_path(handoff_id)?)?;
            let expected_generation = sessions
                .iter()
                .map(|s| s.generation)
                .max()
                .map(|max| {
                    max.checked_add(1)
                        .ok_or(HandoffStoreError::InvalidShellGeneration)
                })
                .transpose()?
                .unwrap_or(1);
            if request.generation != expected_generation {
                return Err(HandoffStoreError::InvalidShellGeneration);
            }
            let session = HandoffShellSession {
                generation: request.generation,
                token_hash: hash_handoff_token(&request.token_plaintext),
                created_at_ms: request.now_ms,
            };
            self.append_jsonl(&self.shell_sessions_path(handoff_id)?, &session)?;
            Ok(session)
        })
    }

    fn list_shell_sessions(
        &self,
        handoff_id: &str,
    ) -> Result<Vec<HandoffShellSession>, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            self.read_jsonl(&self.shell_sessions_path(handoff_id)?)
        })
    }
}

impl CommandCandidateStore for FilesystemHandoffStore {
    fn append_candidate(
        &self,
        handoff_id: &str,
        candidate: &CommandCandidate,
    ) -> Result<(), HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            if candidate.target_handoff_id != handoff_id {
                return Err(HandoffStoreError::InvalidHandoffId);
            }
            self.append_jsonl(&self.candidates_path(handoff_id)?, candidate)
        })
    }

    fn list_candidates(
        &self,
        handoff_id: &str,
    ) -> Result<Vec<CommandCandidate>, HandoffStoreError> {
        self.with_handoff_lock(handoff_id, || {
            self.read_jsonl(&self.candidates_path(handoff_id)?)
        })
    }
}

impl crate::ports::outbound::HandoffAuditRepository for FilesystemHandoffStore {
    fn record_audit(
        &self,
        handoff_id: &str,
        kind: CollaborativeAuditKind,
    ) -> Result<(), HandoffStoreError> {
        self.record_audit(handoff_id, kind)
    }
}

fn same_lease_owner(existing: &HandoffLease, request: &LeaseAcquireRequest) -> bool {
    existing.owner_client_id == request.owner_client_id
        && existing.owner_process_id == request.owner_process_id
        && existing.owner_host == request.owner_host
        && existing.owner_uid == request.owner_uid
}

struct HandoffLock {
    _file: File,
}

impl HandoffLock {
    fn acquire(file: File) -> Result<Self, HandoffStoreError> {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        loop {
            let rc = unsafe { libc::flock(fd, libc::LOCK_EX) };
            if rc == 0 {
                return Ok(Self { _file: file });
            }
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::WouldBlock
                && err.raw_os_error() != Some(libc::EINTR)
            {
                return Err(HandoffStoreError::Write(err.to_string()));
            }
        }
    }
}

impl Drop for HandoffLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        let fd = self._file.as_raw_fd();
        let _ = unsafe { libc::flock(fd, libc::LOCK_UN) };
    }
}

fn read_err(error: impl ToString) -> HandoffStoreError {
    HandoffStoreError::Read(error.to_string())
}

fn write_err(error: impl ToString) -> HandoffStoreError {
    HandoffStoreError::Write(error.to_string())
}

#[cfg(unix)]
fn create_dir_0700(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)
}

#[cfg(unix)]
fn set_permissions_0600(path: &Path) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn create_dir_0700(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(not(unix))]
fn set_permissions_0600(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
