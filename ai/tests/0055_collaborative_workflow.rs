//! 0055 §33 — durable workflow aggregate / reducer / reconciler。
#![cfg(unix)]

use std::sync::atomic::{AtomicUsize, Ordering};

use ai::adapters::outbound::FilesystemHandoffStore;
use ai::application::{
    CollaborativeWorkflowClock, CollaborativeWorkflowEffectExecutor,
    CollaborativeWorkflowReconciler, WorkflowEffectError,
};
use ai::domain::{
    sanitize_workflow_effect_error, ChildGoalAchievement, ChildGoalMeta, CollaborativeWorkflow,
    CollaborativeWorkflowEvent, Handoff, HandoffCheckpoint, HandoffState, PendingWorkflowEffect,
    RequestedShellExec, WorkflowEffectKind, WorkflowEffectState, HANDOFF_SCHEMA_VERSION,
};
use ai::ports::outbound::{
    CollaborativeWorkflowRepository, HandoffShellSessionStore, HandoffStoreError,
    ShellSessionIssueRequest,
};

fn workflow(id: &str) -> CollaborativeWorkflow {
    let handoff = Handoff {
        id: id.into(),
        schema_version: HANDOFF_SCHEMA_VERSION,
        parent_task_id: "task".into(),
        parent_conversation_id: "conversation".into(),
        parent_run_id: "run".into(),
        parent_goal_id: None,
        child_goal_id: "child".into(),
        side_conversation_id: None,
        state: HandoffState::Creating,
        initial_cwd: "/tmp".into(),
        final_shell_cwd: None,
        parent_request_summary: "request".into(),
        requested_shell_execs: vec![],
        pending_human_request: None,
        conversation_snapshot_ref: "snapshot".into(),
        conversation_summary: "summary".into(),
        checkpoint_ref: "workflow.json".into(),
        before_observation_ref: "before".into(),
        after_observation_ref: None,
        shell_log_start: 0,
        shell_log_end: None,
        shell_generation: 0,
        return_reason: None,
        human_shell_exit_code: None,
        resume_error: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    };
    let checkpoint = HandoffCheckpoint {
        parent_task_id: "task".into(),
        parent_conversation_id: "conversation".into(),
        parent_run_id: "run".into(),
        pending_shell_exec: RequestedShellExec {
            command: "true".into(),
            args: vec![],
            cwd: Some("/tmp".into()),
            tool_call_id: Some("tool".into()),
        },
        parent_goal: "goal".into(),
        child_goal: ChildGoalMeta {
            id: "child".into(),
            handoff_id: id.into(),
            parent_goal_id: None,
            work_id: None,
            work_mode: None,
            auto_root_work_id: None,
            close_reason: None,
            close_state: None,
            close_error_message: None,
            achievement: ChildGoalAchievement::Unknown,
        },
        conversation_snapshot: "snapshot".into(),
        conversation_summary: "summary".into(),
        cwd: "/tmp".into(),
        environment_metadata: "{}".into(),
        handoff_id: id.into(),
        side_conversation_id: None,
        command_candidates: vec![],
        shell_log_start: 0,
        control_state: HandoffState::Creating,
        provider_metadata: None,
        tool_executions: vec![],
    };
    CollaborativeWorkflow::new(handoff, checkpoint).unwrap()
}

#[test]
fn workflow_atomic_aggregate_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(tmp.path().join("handoffs"));
    let mut value = workflow("atomic");
    value
        .apply(CollaborativeWorkflowEvent::EnqueueEffect(
            PendingWorkflowEffect::pending("effect-1", WorkflowEffectKind::ReleaseLease, 1),
        ))
        .unwrap();
    store.create_workflow(&value).unwrap();

    let loaded = store.load_workflow("atomic").unwrap();
    assert_eq!(loaded, value);
    assert!(tmp.path().join("handoffs/atomic/workflow.json").is_file());
    assert!(!tmp.path().join("handoffs/atomic/checkpoint.json").exists());
}

#[test]
fn workflow_reducer_rejects_invariant_violation() {
    let mut value = workflow("invariant");
    let before = value.clone();
    let mut mismatched = value.checkpoint.clone();
    mismatched.control_state = HandoffState::Returned;
    assert!(value
        .apply(CollaborativeWorkflowEvent::ReplaceCheckpoint(Box::new(
            mismatched,
        )))
        .is_err());
    assert_eq!(value, before);
}

struct Clock;
impl CollaborativeWorkflowClock for Clock {
    fn now_ms(&self) -> u64 {
        1_000
    }
}

struct Executor(AtomicUsize);
impl CollaborativeWorkflowEffectExecutor for Executor {
    fn execute(
        &self,
        _workflow: &CollaborativeWorkflow,
        _effect: &PendingWorkflowEffect,
    ) -> Result<(), WorkflowEffectError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn workflow_reconciler_retries_pending_effect_once() {
    let tmp = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(tmp.path().join("handoffs"));
    let mut value = workflow("reconcile");
    let mut effect =
        PendingWorkflowEffect::pending("release-lease", WorkflowEffectKind::ReleaseLease, 1);
    effect.state = WorkflowEffectState::InFlight;
    effect.claimed_at_ms = Some(1);
    value.pending_effects.push(effect);
    store.create_workflow(&value).unwrap();
    let executor = Executor(AtomicUsize::new(0));
    let reconciler = CollaborativeWorkflowReconciler::new(&store, &executor, &Clock, 100);

    let report = reconciler.reconcile("reconcile").unwrap();
    assert_eq!(report.completed_effects, vec!["release-lease"]);
    assert_eq!(executor.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.load_workflow("reconcile").unwrap().pending_effects[0].state,
        WorkflowEffectState::Completed
    );
}

struct PeerClaimRaceStore {
    inner: FilesystemHandoffStore,
}

impl PeerClaimRaceStore {
    fn new(root: std::path::PathBuf) -> Self {
        Self {
            inner: FilesystemHandoffStore::new(root),
        }
    }
}

impl CollaborativeWorkflowRepository for PeerClaimRaceStore {
    fn create_workflow(&self, workflow: &CollaborativeWorkflow) -> Result<(), HandoffStoreError> {
        self.inner.create_workflow(workflow)
    }

    fn load_workflow(&self, handoff_id: &str) -> Result<CollaborativeWorkflow, HandoffStoreError> {
        self.inner.load_workflow(handoff_id)
    }

    fn list_workflows(&self) -> Result<Vec<CollaborativeWorkflow>, HandoffStoreError> {
        self.inner.list_workflows()
    }

    fn compare_and_swap_workflow(
        &self,
        expected_revision: u64,
        workflow: &CollaborativeWorkflow,
    ) -> Result<(), HandoffStoreError> {
        self.inner
            .compare_and_swap_workflow(expected_revision, workflow)
    }

    fn mutate_workflow(
        &self,
        handoff_id: &str,
        mutation: &mut dyn FnMut(&mut CollaborativeWorkflow) -> Result<(), HandoffStoreError>,
    ) -> Result<CollaborativeWorkflow, HandoffStoreError> {
        let before = self.inner.load_workflow(handoff_id)?;
        let peer_claim = before.pending_effects.iter().any(|effect| {
            effect.id == "release-lease" && effect.state == WorkflowEffectState::Pending
        });
        if peer_claim {
            self.inner.mutate_workflow(handoff_id, &mut |workflow| {
                workflow
                    .apply(CollaborativeWorkflowEvent::ClaimEffect {
                        effect_id: "release-lease".into(),
                        now_ms: 500,
                    })
                    .map_err(HandoffStoreError::from)
            })?;
        }
        self.inner.mutate_workflow(handoff_id, mutation)
    }
}

#[test]
fn workflow_reconciler_skips_effect_already_claimed_by_peer() {
    let tmp = tempfile::tempdir().unwrap();
    let store = PeerClaimRaceStore::new(tmp.path().join("handoffs"));
    let mut value = workflow("peer-claim");
    value
        .apply(CollaborativeWorkflowEvent::EnqueueEffect(
            PendingWorkflowEffect::pending("release-lease", WorkflowEffectKind::ReleaseLease, 1),
        ))
        .unwrap();
    store.create_workflow(&value).unwrap();
    let executor = Executor(AtomicUsize::new(0));
    let reconciler = CollaborativeWorkflowReconciler::new(&store, &executor, &Clock, 100);

    let report = reconciler.reconcile("peer-claim").unwrap();
    assert!(report.completed_effects.is_empty());
    assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        store.load_workflow("peer-claim").unwrap().pending_effects[0].state,
        WorkflowEffectState::InFlight
    );
}

struct SecretFailingExecutor;
impl CollaborativeWorkflowEffectExecutor for SecretFailingExecutor {
    fn execute(
        &self,
        _workflow: &CollaborativeWorkflow,
        _effect: &PendingWorkflowEffect,
    ) -> Result<(), WorkflowEffectError> {
        Err(WorkflowEffectError {
            message: "Authorization: Bearer secret123 failed".into(),
            retryable: true,
        })
    }
}

#[test]
fn workflow_effect_error_sanitizer_never_persists_secrets() {
    assert_eq!(
        sanitize_workflow_effect_error(
            "Authorization: Bearer abc123 failed for token sk-live-secret",
        ),
        "effect_failed"
    );
    assert_eq!(
        sanitize_workflow_effect_error("candidate not found in checkpoint"),
        "resource_not_found"
    );

    let tmp = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(tmp.path().join("handoffs"));
    let mut value = workflow("sanitize");
    value
        .apply(CollaborativeWorkflowEvent::EnqueueEffect(
            PendingWorkflowEffect::pending("release-lease", WorkflowEffectKind::ReleaseLease, 1),
        ))
        .unwrap();
    store.create_workflow(&value).unwrap();
    let reconciler =
        CollaborativeWorkflowReconciler::new(&store, &SecretFailingExecutor, &Clock, 100);
    reconciler.reconcile("sanitize").unwrap();
    let loaded = store.load_workflow("sanitize").unwrap();
    let last_error = loaded.pending_effects[0]
        .last_error
        .as_deref()
        .expect("retryable failure persists sanitized last_error");
    assert_eq!(last_error, "effect_failed");
    let raw = std::fs::read_to_string(tmp.path().join("handoffs/sanitize/workflow.json")).unwrap();
    assert!(!raw.contains("Bearer"));
    assert!(!raw.contains("secret123"));
}

#[test]
fn workflow_never_persists_plaintext_token() {
    let tmp = tempfile::tempdir().unwrap();
    let store = FilesystemHandoffStore::new(tmp.path().join("handoffs"));
    let value = workflow("token");
    store.create_workflow(&value).unwrap();
    store
        .append_shell_session(
            "token",
            &ShellSessionIssueRequest {
                generation: 1,
                token_plaintext: "super-secret-token".into(),
                now_ms: 1,
            },
        )
        .unwrap();
    for entry in std::fs::read_dir(tmp.path().join("handoffs/token")).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(path).unwrap_or_default();
        assert!(!raw.contains("super-secret-token"));
        assert!(!raw.contains("token_plaintext"));
    }
}
