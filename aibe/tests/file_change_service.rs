//! `FileChangeService` 単体テスト（0054 Phase 5）。

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::{
    ConfigWritePathRevalidator, FileChangeJournalConfig, FilesystemFileChangeJournal,
    FilesystemFileChangeStore,
};
use aibe::application::file_change_service::FileChangeService;
use aibe::domain::{
    sanitize_apply_patch_arguments, sanitize_write_file_arguments, sha256_hex, ClientCwd,
    FileChangeOperation, FileSnapshot,
};
use aibe::ports::outbound::{
    FileChangeError, FileChangeExecuteRequest, FileChangeExecutor, FileWriteApprovalMode,
    FileWriteConfig, ToolApprovalGate, ToolApprovalGateOutcome, ToolApprovalPromptRequest,
    ToolExecutionContext, TurnCancellation,
};
use aibe_protocol::ToolApprovalOrigin;
use async_trait::async_trait;
use tempfile::tempdir;
use tokio::sync::Mutex;

pub struct TestToolApprovalGate {
    response: Mutex<ToolApprovalGateOutcome>,
    delay: Option<Duration>,
    prompt_count: AtomicUsize,
    cancellation: Option<Arc<TurnCancellation>>,
}

impl TestToolApprovalGate {
    pub fn fixed(response: ToolApprovalGateOutcome) -> Self {
        Self {
            response: Mutex::new(response),
            delay: None,
            prompt_count: AtomicUsize::new(0),
            cancellation: None,
        }
    }

    pub fn delayed(response: ToolApprovalGateOutcome, delay: Duration) -> Self {
        Self {
            response: Mutex::new(response),
            delay: Some(delay),
            prompt_count: AtomicUsize::new(0),
            cancellation: None,
        }
    }

    pub fn delayed_with_cancellation(
        response: ToolApprovalGateOutcome,
        delay: Duration,
        cancellation: Arc<TurnCancellation>,
    ) -> Self {
        Self {
            response: Mutex::new(response),
            delay: Some(delay),
            prompt_count: AtomicUsize::new(0),
            cancellation: Some(cancellation),
        }
    }

    pub fn prompt_count(&self) -> usize {
        self.prompt_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ToolApprovalGate for TestToolApprovalGate {
    async fn request_tool_approval(
        &self,
        _tool_call_id: &str,
        _prompt: ToolApprovalPromptRequest,
    ) -> ToolApprovalGateOutcome {
        self.prompt_count.fetch_add(1, Ordering::SeqCst);
        if let Some(delay) = self.delay {
            let step = Duration::from_millis(20);
            let mut waited = Duration::ZERO;
            while waited < delay {
                if let Some(cancel) = &self.cancellation {
                    if cancel.is_cancelled() {
                        return ToolApprovalGateOutcome::Cancelled;
                    }
                }
                let slice = step.min(delay - waited);
                tokio::time::sleep(slice).await;
                waited += slice;
            }
        }
        *self.response.lock().await
    }
}

pub fn test_ctx(
    gate: Option<Arc<dyn ToolApprovalGate>>,
    cwd: &std::path::Path,
) -> ToolExecutionContext {
    let cwd = ClientCwd::new(cwd.to_path_buf()).expect("cwd");
    let mut ctx = ToolExecutionContext::new(cwd).with_turn_id("turn-test");
    if let Some(gate) = gate {
        ctx = ctx.with_tool_approval_gate(gate);
    }
    ctx
}

pub fn test_service(config: FileWriteConfig, journal_root: PathBuf) -> FileChangeService {
    let journal = Arc::new(FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: journal_root,
        retention_days: 7,
        max_bytes: 1_000_000,
    }));
    let store = Arc::new(FilesystemFileChangeStore);
    let path_revalidator = Arc::new(ConfigWritePathRevalidator::from_config(&config));
    FileChangeService::new(config, journal, store, path_revalidator)
}

pub fn create_request(
    path: PathBuf,
    operation: FileChangeOperation,
    before: FileSnapshot,
    after: &[u8],
    sanitized: serde_json::Value,
) -> FileChangeExecuteRequest {
    let display = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let plan = FileChangeService::prepare_plan(
        path.clone(),
        operation,
        before,
        after.to_vec(),
        &display,
        8_192,
    );
    FileChangeExecuteRequest {
        tool_call_id: "call-1".into(),
        tool_name: "write_file".into(),
        plan,
        sanitized_arguments: sanitized,
    }
}

#[tokio::test]
async fn file_change_prepare_does_not_mutate_file() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("new.txt");
    let before = std::fs::metadata(dir.path()).expect("meta");
    let _plan = FileChangeService::prepare_plan(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        b"hello\n".to_vec(),
        "new.txt",
        8_192,
    );
    assert!(!path.exists());
    let after = std::fs::metadata(dir.path()).expect("meta");
    assert_eq!(
        before.modified().expect("mtime"),
        after.modified().expect("mtime")
    );
}

#[tokio::test]
async fn file_change_fake_gate_yes_commits() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let gate = Arc::new(TestToolApprovalGate::fixed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
    ));
    let service = test_service(FileWriteConfig::default(), dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        b"hello\n",
        sanitize_write_file_arguments("out.txt", "create", None, "hello\n"),
    );
    let result = service
        .execute(request, &test_ctx(Some(gate), dir.path()), None, None)
        .await
        .expect("commit");
    assert!(path.is_file());
    assert_eq!(std::fs::read(&path).expect("read"), b"hello\n");
    assert!(result.change_id.starts_with("chg_"));
}

#[tokio::test]
async fn file_change_fake_gate_no_leaves_file_unchanged() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    std::fs::write(&path, b"original\n").expect("seed");
    let gate = Arc::new(TestToolApprovalGate::fixed(
        ToolApprovalGateOutcome::Denied(ToolApprovalOrigin::UiNo),
    ));
    let service = test_service(FileWriteConfig::default(), dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Replace,
        FileSnapshot::present(
            b"original\n".to_vec(),
            sha256_hex(b"original\n"),
            Some(0o644),
        ),
        b"new\n",
        sanitize_write_file_arguments(
            "out.txt",
            "replace",
            Some(&sha256_hex(b"original\n")),
            "new\n",
        ),
    );
    let (err, _) = service
        .execute(request, &test_ctx(Some(gate), dir.path()), None, None)
        .await
        .expect_err("denied");
    assert_eq!(err, FileChangeError::ApprovalDenied);
    assert_eq!(std::fs::read(&path).expect("read"), b"original\n");
}

#[tokio::test]
async fn file_write_never_mode_denies_execution() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let gate = Arc::new(TestToolApprovalGate::fixed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
    ));
    let mut config = FileWriteConfig::default();
    config.approval = FileWriteApprovalMode::Never;
    let service = test_service(config, dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        b"hello\n",
        sanitize_write_file_arguments("out.txt", "create", None, "hello\n"),
    );
    let (err, executed) = service
        .execute(request, &test_ctx(Some(gate), dir.path()), None, None)
        .await
        .expect_err("denied");
    assert_eq!(err, FileChangeError::WriteDeniedByPolicy);
    assert_eq!(executed.error.as_deref(), Some("write_denied_by_policy"));
    assert!(!path.exists());
}

#[tokio::test]
async fn file_write_always_mode_skips_prompt() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let gate = Arc::new(TestToolApprovalGate::fixed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
    ));
    let mut config = FileWriteConfig::default();
    config.approval = FileWriteApprovalMode::Always;
    let service = test_service(config, dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        b"hello\n",
        sanitize_write_file_arguments("out.txt", "create", None, "hello\n"),
    );
    service
        .execute(
            request,
            &test_ctx(Some(gate.clone()), dir.path()),
            None,
            None,
        )
        .await
        .expect("commit");
    assert_eq!(gate.prompt_count(), 0);
    assert!(path.is_file());
}

#[tokio::test]
async fn file_write_disabled_returns_tool_disabled() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let mut config = FileWriteConfig::default();
    config.enabled = false;
    let service = test_service(config, dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        b"hello\n",
        sanitize_write_file_arguments("out.txt", "create", None, "hello\n"),
    );
    let (err, executed) = service
        .execute(request, &test_ctx(None, dir.path()), None, None)
        .await
        .expect_err("disabled");
    assert_eq!(err, FileChangeError::ToolDisabled);
    assert_eq!(executed.error.as_deref(), Some("tool_disabled"));
}

#[tokio::test]
async fn file_change_missing_gate_returns_unavailable() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let service = test_service(FileWriteConfig::default(), dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        b"hello\n",
        sanitize_write_file_arguments("out.txt", "create", None, "hello\n"),
    );
    let (err, executed) = service
        .execute(request, &test_ctx(None, dir.path()), None, None)
        .await
        .expect_err("unavailable");
    assert_eq!(err, FileChangeError::ApprovalUnavailable);
    assert_eq!(executed.error.as_deref(), Some("approval_unavailable"));
}

#[tokio::test]
async fn file_change_cancel_during_approval_writes_nothing() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let cancel = Arc::new(TurnCancellation::new());
    let gate = Arc::new(TestToolApprovalGate::delayed_with_cancellation(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
        Duration::from_millis(200),
        Arc::clone(&cancel),
    ));
    let service = test_service(FileWriteConfig::default(), dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        b"hello\n",
        sanitize_write_file_arguments("out.txt", "create", None, "hello\n"),
    );
    let cancel_clone = Arc::clone(&cancel);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel_clone.cancel();
    });
    let (err, _) = service
        .execute(
            request,
            &test_ctx(Some(gate), dir.path()),
            Some(&cancel),
            None,
        )
        .await
        .expect_err("cancelled");
    assert_eq!(err, FileChangeError::Cancelled);
    assert!(!path.exists());
}

#[tokio::test]
async fn file_change_revalidate_detects_stale_file() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    std::fs::write(&path, b"original\n").expect("seed");
    let gate = Arc::new(TestToolApprovalGate::delayed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
        Duration::from_millis(150),
    ));
    let path_for_race = path.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(&path_for_race, b"tampered\n").expect("tamper");
    });
    let service = test_service(FileWriteConfig::default(), dir.path().join("journal"));
    let request = create_request(
        path.clone(),
        FileChangeOperation::Replace,
        FileSnapshot::present(
            b"original\n".to_vec(),
            sha256_hex(b"original\n"),
            Some(0o644),
        ),
        b"new\n",
        sanitize_write_file_arguments(
            "out.txt",
            "replace",
            Some(&sha256_hex(b"original\n")),
            "new\n",
        ),
    );
    let (err, _) = service
        .execute(request, &test_ctx(Some(gate), dir.path()), None, None)
        .await
        .expect_err("stale");
    assert_eq!(err, FileChangeError::StaleFile);
    assert_eq!(std::fs::read(&path).expect("read"), b"tampered\n");
}

#[tokio::test]
async fn file_change_sanitizes_executed_tool_arguments() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    let gate = Arc::new(TestToolApprovalGate::fixed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
    ));
    let mut config = FileWriteConfig::default();
    config.approval = FileWriteApprovalMode::Always;
    let service = test_service(config, dir.path().join("journal"));
    let secret = "super-secret-content\n";
    let sanitized = sanitize_write_file_arguments("out.txt", "create", None, secret);
    let request = create_request(
        path.clone(),
        FileChangeOperation::Create,
        FileSnapshot::absent(),
        secret.as_bytes(),
        sanitized.clone(),
    );
    let patch_sanitized = sanitize_apply_patch_arguments("out.txt", "abc123", "@@ patch body", 1);
    assert!(!patch_sanitized.to_string().contains("patch body"));

    let result = service
        .execute(request, &test_ctx(Some(gate), dir.path()), None, None)
        .await
        .expect("commit");
    let args = result.executed.arguments.to_string();
    assert!(!args.contains("super-secret-content"));
    assert!(args.contains("content_bytes"));
    assert_eq!(result.executed.arguments, sanitized);
}
