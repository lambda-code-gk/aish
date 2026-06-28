//! Work state filesystem adapter。

use std::fs;
use std::path::{Path, PathBuf};

use aibe_protocol::is_valid_memory_space_id;

use super::memory_space_fs::{acquire_space_lock, atomic_replace_0600, space_dir};
use crate::domain::WorkState;
use crate::ports::outbound::{WorkStore, WorkStoreContext, WorkStoreError};

#[derive(Debug, Clone)]
pub struct FilesystemWorkStore {
    aibe_root: PathBuf,
}

impl FilesystemWorkStore {
    pub fn new(aibe_root: PathBuf) -> Self {
        Self { aibe_root }
    }

    pub fn with_conversation_root(conversation_root: PathBuf) -> Self {
        let aibe_root = conversation_root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(conversation_root);
        Self::new(aibe_root)
    }

    fn state_path(&self, memory_space_id: &str) -> PathBuf {
        space_dir(&self.aibe_root, memory_space_id).join("work-state.json")
    }

    fn validate_context(ctx: &WorkStoreContext) -> Result<(), WorkStoreError> {
        if is_valid_memory_space_id(&ctx.memory_space_id) {
            Ok(())
        } else {
            Err(WorkStoreError::InvalidMemorySpace)
        }
    }

    fn load_unlocked(&self, ctx: &WorkStoreContext) -> Result<WorkState, WorkStoreError> {
        let path = self.state_path(&ctx.memory_space_id);
        if !path.exists() {
            return Ok(WorkState::default());
        }
        let bytes = fs::read(&path).map_err(|error| WorkStoreError::Io(error.to_string()))?;
        let state: WorkState = serde_json::from_slice(&bytes)
            .map_err(|error| WorkStoreError::Corrupt(error.to_string()))?;
        state
            .validate()
            .map_err(|error| WorkStoreError::Validation(error.to_string()))?;
        Ok(state)
    }
}

impl WorkStore for FilesystemWorkStore {
    fn load(&self, ctx: &WorkStoreContext) -> Result<WorkState, WorkStoreError> {
        Self::validate_context(ctx)?;
        let _guard = acquire_space_lock(&self.aibe_root, &ctx.memory_space_id)
            .map_err(|error| WorkStoreError::Io(error.to_string()))?;
        self.load_unlocked(ctx)
    }

    fn mutate(
        &self,
        ctx: &WorkStoreContext,
        mutation: &mut dyn FnMut(&mut WorkState) -> Result<(), WorkStoreError>,
    ) -> Result<WorkState, WorkStoreError> {
        Self::validate_context(ctx)?;
        let _guard = acquire_space_lock(&self.aibe_root, &ctx.memory_space_id)
            .map_err(|error| WorkStoreError::Io(error.to_string()))?;
        let mut state = self.load_unlocked(ctx)?;
        mutation(&mut state)?;
        state
            .validate()
            .map_err(|error| WorkStoreError::Validation(error.to_string()))?;
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| WorkStoreError::Validation("revision overflow".into()))?;
        let bytes = serde_json::to_vec_pretty(&state)
            .map_err(|error| WorkStoreError::Corrupt(error.to_string()))?;
        atomic_replace_0600(&self.state_path(&ctx.memory_space_id), &bytes)
            .map_err(|error| WorkStoreError::Io(error.to_string()))?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use aibe_protocol::WORK_SCHEMA_VERSION;

    use super::*;
    use crate::domain::{WorkItem, WorkStatus};

    fn add_deferred(state: &mut WorkState, title: String) -> Result<(), WorkStoreError> {
        let id = state
            .allocate_work_id()
            .map_err(|error| WorkStoreError::Mutation(error.to_string()))?;
        state.works.push(WorkItem {
            id,
            title: title.clone(),
            goal: title,
            status: WorkStatus::Deferred,
            parent_id: None,
            created_at_ms: id,
            updated_at_ms: id,
            finished_at_ms: None,
            focus: None,
            summary: None,
        });
        Ok(())
    }

    #[test]
    fn work_store_persists_atomic_state_and_preserves_corrupt_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = FilesystemWorkStore::new(root.path().to_path_buf());
        let ctx = WorkStoreContext {
            memory_space_id: "project_test".into(),
        };
        let first = store
            .mutate(&ctx, &mut |state| add_deferred(state, "first".into()))
            .expect("first mutation");
        let second = store
            .mutate(&ctx, &mut |state| add_deferred(state, "second".into()))
            .expect("second mutation");
        assert_eq!(first.works[0].id, 1);
        assert_eq!(second.works[1].id, 2);
        assert_eq!(store.load(&ctx).expect("reload"), second);

        let state_path = store.state_path(&ctx.memory_space_id);
        assert_eq!(
            fs::metadata(&state_path)
                .expect("state metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(state_path.parent().expect("space dir"))
                .expect("dir metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(state_path.parent().expect("space dir").join(".lock"),)
                .expect("lock metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        fs::write(&state_path, b"{broken").expect("corrupt state");
        let original = fs::read(&state_path).expect("read corrupt state");
        assert!(store.load(&ctx).is_err());
        assert!(store
            .mutate(&ctx, &mut |state| add_deferred(state, "third".into()))
            .is_err());
        assert_eq!(
            fs::read(&state_path).expect("re-read corrupt state"),
            original
        );

        for invalid in [
            serde_json::json!({
                "schema_version": WORK_SCHEMA_VERSION + 1,
                "revision": 0,
                "next_work_id": 1,
                "active_work_id": null,
                "stack": [],
                "works": [],
                "entries": []
            }),
            serde_json::json!({
                "schema_version": WORK_SCHEMA_VERSION,
                "revision": 0,
                "next_work_id": 1,
                "active_work_id": 1,
                "stack": [],
                "works": [],
                "entries": []
            }),
            serde_json::json!({
                "schema_version": WORK_SCHEMA_VERSION,
                "revision": 0,
                "next_work_id": 2,
                "active_work_id": null,
                "stack": [],
                "works": [{
                    "id": 1,
                    "title": "valid title",
                    "goal": "bad\u{0000}goal",
                    "status": "deferred",
                    "parent_id": null,
                    "created_at_ms": 1,
                    "updated_at_ms": 1,
                    "finished_at_ms": null,
                    "focus": null,
                    "summary": null
                }],
                "entries": []
            }),
            serde_json::json!({
                "schema_version": WORK_SCHEMA_VERSION,
                "revision": 0,
                "next_work_id": 2,
                "active_work_id": null,
                "stack": [],
                "works": [{
                    "id": 1,
                    "title": "orphan",
                    "goal": "orphan",
                    "status": "deferred",
                    "parent_id": 99,
                    "created_at_ms": 1,
                    "updated_at_ms": 1,
                    "finished_at_ms": null,
                    "focus": null,
                    "summary": null
                }],
                "entries": []
            }),
            serde_json::json!({
                "schema_version": WORK_SCHEMA_VERSION,
                "revision": 0,
                "next_work_id": 2,
                "active_work_id": null,
                "stack": [],
                "works": [{
                    "id": 1,
                    "title": "zero parent",
                    "goal": "zero parent",
                    "status": "deferred",
                    "parent_id": 0,
                    "created_at_ms": 1,
                    "updated_at_ms": 1,
                    "finished_at_ms": null,
                    "focus": null,
                    "summary": null
                }],
                "entries": []
            }),
            serde_json::json!({
                "schema_version": WORK_SCHEMA_VERSION,
                "revision": 0,
                "next_work_id": 3,
                "active_work_id": null,
                "stack": [],
                "works": [
                    {
                        "id": 1,
                        "title": "cycle one",
                        "goal": "cycle one",
                        "status": "paused",
                        "parent_id": 2,
                        "created_at_ms": 1,
                        "updated_at_ms": 1,
                        "finished_at_ms": null,
                        "focus": null,
                        "summary": null
                    },
                    {
                        "id": 2,
                        "title": "cycle two",
                        "goal": "cycle two",
                        "status": "paused",
                        "parent_id": 1,
                        "created_at_ms": 2,
                        "updated_at_ms": 2,
                        "finished_at_ms": null,
                        "focus": null,
                        "summary": null
                    }
                ],
                "entries": []
            }),
        ] {
            let invalid = serde_json::to_vec(&invalid).expect("serialize invalid state");
            fs::write(&state_path, &invalid).expect("write invalid state");
            assert!(store.load(&ctx).is_err());
            assert!(store
                .mutate(&ctx, &mut |state| add_deferred(state, "rejected".into()))
                .is_err());
            assert_eq!(
                fs::read(&state_path).expect("re-read invalid state"),
                invalid
            );
        }
    }

    #[test]
    fn work_store_serializes_concurrent_mutations_without_lost_updates() {
        let root = tempfile::tempdir().expect("tempdir");
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for worker in 0..2 {
            let store = FilesystemWorkStore::new(root.path().to_path_buf());
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                let ctx = WorkStoreContext {
                    memory_space_id: "project_concurrent".into(),
                };
                barrier.wait();
                for index in 0..10 {
                    store
                        .mutate(&ctx, &mut |state| {
                            add_deferred(state, format!("worker-{worker}-{index}"))
                        })
                        .expect("concurrent mutation");
                }
            }));
        }
        barrier.wait();
        for handle in handles {
            handle.join().expect("worker join");
        }
        let store = FilesystemWorkStore::new(root.path().to_path_buf());
        let state = store
            .load(&WorkStoreContext {
                memory_space_id: "project_concurrent".into(),
            })
            .expect("load final state");
        assert_eq!(state.works.len(), 20);
        assert_eq!(state.next_work_id, 21);
        assert_eq!(state.revision, 20);
    }
}
