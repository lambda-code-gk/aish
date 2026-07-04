// RED stubs for 0054 Safe File Write Tools.
// Removed from #[ignore] when the corresponding phase lands.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::tools::{
    atomic_write_file, build_unified_diff_preview, dir_has_temp_leftovers, ApplyPatchTool,
    DefaultToolRegistry, ReadFileTool, ReadPathPolicy, WriteFileTool, WritePathPolicy,
    FILE_METADATA_PREFIX,
};
use aibe::adapters::outbound::{
    path_mode, read_journal_metadata, set_journal_created_at_for_test, ConfigWritePathRevalidator,
    FileChangeJournalConfig, FilesystemFileChangeJournal, FilesystemFileChangeStore,
    StaticCapabilityPolicy,
};
use aibe::application::file_change_service::FileChangeService;
use aibe::application::tool_round::{RoundOutcome, ToolRoundExecutor};
use aibe::domain::{
    check_file_size, detect_line_ending, sha256_hex, validate_utf8_bytes, BeforeState, Capability,
    ChatMessage, ClientCwd, FileChangeOperation, FileTextError, LineEnding, LlmStepResult,
    ToolCall, ToolName,
};
use aibe::ports::outbound::FileChangeExecutor;
use aibe::ports::outbound::{
    FileChangeJournal, FileChangeJournalError, FileChangeStore, FileChangeStoreError,
    FileWriteApprovalMode, FileWriteConfig, JournalSaveRequest, LlmProvider, NoopLlmCallTracer,
    ReadFileConfig, ToolApprovalGate, ToolApprovalGateOutcome, ToolApprovalPromptRequest,
    ToolDefinition, ToolExecutionContext, ToolExecutor, ToolsConfig,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::tempdir;
use tokio::sync::Mutex;

use aibe::ports::outbound::{
    DEFAULT_JOURNAL_MAX_BYTES, DEFAULT_JOURNAL_RETENTION_DAYS, DEFAULT_MAX_FILE_WRITE_BYTES,
    DEFAULT_MAX_PATCH_BYTES, DEFAULT_MAX_PREVIEW_BYTES,
};

#[test]
fn file_write_capability_roundtrip() {
    let cap = Capability::FileWrite;
    assert_eq!(cap.as_str(), "file:write");
    assert_eq!(Capability::parse_wire("file:write"), Some(cap));
    assert_eq!(Capability::parse_wire(cap.as_str()), Some(cap));
}

#[test]
fn file_write_config_defaults_match_spec() {
    let cfg = FileWriteConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.allowed_roots, vec![PathBuf::from(".")]);
    assert_eq!(cfg.approval, FileWriteApprovalMode::Ask);
    assert_eq!(cfg.max_file_bytes, DEFAULT_MAX_FILE_WRITE_BYTES);
    assert_eq!(cfg.max_patch_bytes, DEFAULT_MAX_PATCH_BYTES);
    assert_eq!(cfg.max_preview_bytes, DEFAULT_MAX_PREVIEW_BYTES);
    assert_eq!(cfg.journal_retention_days, DEFAULT_JOURNAL_RETENTION_DAYS);
    assert_eq!(cfg.journal_max_bytes, DEFAULT_JOURNAL_MAX_BYTES);

    let tools = ToolsConfig::default();
    assert!(tools.file_write.enabled);
    assert_eq!(tools.file_write.allowed_roots, cfg.allowed_roots);
    assert_eq!(tools.file_write.approval, cfg.approval);
    assert_eq!(tools.file_write.max_file_bytes, cfg.max_file_bytes);
    assert_eq!(tools.file_write.max_patch_bytes, cfg.max_patch_bytes);
    assert_eq!(tools.file_write.max_preview_bytes, cfg.max_preview_bytes);
    assert_eq!(
        tools.file_write.journal_retention_days,
        cfg.journal_retention_days
    );
    assert_eq!(tools.file_write.journal_max_bytes, cfg.journal_max_bytes);
}

struct FixedNameTool {
    name: ToolName,
}

#[async_trait]
impl ToolExecutor for FixedNameTool {
    fn name(&self) -> ToolName {
        self.name.clone()
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        _arguments: &Value,
        _timeout_ms: u64,
        _ctx: &aibe::ports::outbound::ToolExecutionContext,
    ) -> (aibe::domain::ExecutedToolCall, aibe::domain::ToolResult) {
        (
            aibe::domain::ExecutedToolCall::ok(
                tool_call_id.to_string(),
                self.name.as_str(),
                Value::Null,
                String::new(),
            ),
            aibe::domain::ToolResult {
                tool_call_id: tool_call_id.to_string(),
                content: String::new(),
                is_error: false,
            },
        )
    }
}

#[test]
fn tool_registry_rejects_duplicate_tool_name() {
    let duplicate = Arc::new(FixedNameTool {
        name: ToolName::read_file(),
    });
    let err = match DefaultToolRegistry::from_executors([
        Arc::clone(&duplicate) as Arc<dyn ToolExecutor>,
        duplicate,
    ]) {
        Ok(_) => panic!("duplicate tool names must be rejected"),
        Err(err) => err,
    };
    assert_eq!(err.0, "read_file");
}

#[test]
fn file_size_limit_enforced() {
    assert!(check_file_size(1024, 1024).is_ok());
    assert_eq!(
        check_file_size(1025, 1024),
        Err(FileTextError::FileTooLarge)
    );
}

#[test]
fn line_ending_detection_covers_all_kinds() {
    assert_eq!(detect_line_ending("a\nb\n"), LineEnding::Lf);
    assert_eq!(detect_line_ending("a\r\nb\r\n"), LineEnding::Crlf);
    assert_eq!(detect_line_ending("plain"), LineEnding::None);
    assert_eq!(detect_line_ending("a\nb\r\nc"), LineEnding::Mixed);
}

#[tokio::test]
async fn read_file_uses_shared_safe_path_resolver() {
    let dir = tempdir().expect("tempdir");
    let allowed = dir.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("mkdir");
    std::fs::write(allowed.join("note.txt"), "shared resolver").expect("write");

    let policy = ReadPathPolicy::new(vec![allowed.clone()]);
    let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
    let via_policy = policy
        .resolve_read_path(&ctx, Path::new("allowed/note.txt"))
        .await
        .expect("policy resolve");

    let tool = aibe::adapters::outbound::tools::ReadFileTool::new(
        ReadFileConfig {
            allowed_roots: vec![allowed],
        },
        4096,
    );
    let args = serde_json::json!({ "path": "allowed/note.txt" });
    let (_record, result) = tool.execute("tc-read", &args, 5000, &ctx).await;
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(result.content, "shared resolver");
    assert_eq!(
        via_policy,
        dir.path()
            .join("allowed/note.txt")
            .canonicalize()
            .expect("canon")
    );
}

#[tokio::test]
async fn write_roots_are_independent_from_read_roots() {
    let dir = tempdir().expect("tempdir");
    let read_root = dir.path().join("read_area");
    let write_root = dir.path().join("write_area");
    std::fs::create_dir_all(&read_root).expect("mkdir read");
    std::fs::create_dir_all(&write_root).expect("mkdir write");
    std::fs::write(read_root.join("only_read.txt"), "secret").expect("write");

    let read_policy = ReadPathPolicy::new(vec![read_root]);
    let write_policy = WritePathPolicy::new(vec![write_root]);
    let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));

    read_policy
        .resolve_read_path(&ctx, Path::new("read_area/only_read.txt"))
        .await
        .expect("read should allow read root");

    let write_err = write_policy
        .resolve_write_path(&ctx, Path::new("read_area/only_read.txt"))
        .await
        .expect_err("write must not reuse read roots");
    assert_eq!(write_err.code, "path_not_allowed");
}

#[test]
fn sha256_hashes_file_bytes() {
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    assert_eq!(sha256_hex(b""), sha256_hex(&[]));
    assert_eq!(sha256_hex(b"abc").len(), 64);
}

#[test]
fn text_validation_rejects_binary_and_invalid_utf8() {
    assert!(validate_utf8_bytes(b"text").is_ok());
    assert_eq!(
        validate_utf8_bytes(&[0xff, 0xfe, 0x00]),
        Err(FileTextError::BinaryFileNotSupported)
    );
    assert_eq!(
        validate_utf8_bytes(&[0xff, 0xfe]),
        Err(FileTextError::InvalidUtf8)
    );
}

#[tokio::test]
async fn write_path_resolves_under_allowed_roots() {
    let dir = tempdir().expect("tempdir");
    let write_root = dir.path().join("writable");
    std::fs::create_dir_all(&write_root).expect("mkdir");
    std::fs::write(write_root.join("note.txt"), "ok").expect("write");

    let policy = WritePathPolicy::new(vec![write_root.clone()]);
    let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
    let got = policy
        .resolve_write_path(&ctx, Path::new("writable/note.txt"))
        .await
        .expect("resolve");
    assert_eq!(
        got,
        write_root.join("note.txt").canonicalize().expect("canon")
    );
}

#[test]
fn write_path_rejects_parent_components() {
    let err = WritePathPolicy::validate_path_string("../outside").unwrap_err();
    assert_eq!(err.code, "path_not_allowed");
    assert!(err.message.contains("'..'"));
}

#[cfg(unix)]
#[tokio::test]
async fn write_path_rejects_symlinks() {
    use std::os::unix::fs::symlink;

    let base = tempdir().expect("base");
    let outside = tempdir().expect("outside");
    std::fs::write(outside.path().join("secret.txt"), "secret").expect("write");
    symlink(outside.path(), base.path().join("link")).expect("symlink");

    let policy = WritePathPolicy::new(vec![base.path().to_path_buf()]);
    let ctx = ToolExecutionContext::new(ClientCwd::new(base.path().to_path_buf()).expect("cwd"));
    let err = policy
        .resolve_write_path(&ctx, Path::new("link/secret.txt"))
        .await
        .expect_err("symlink");
    assert_eq!(err.code, "symlink_not_allowed");
}

#[cfg(unix)]
#[tokio::test]
async fn write_path_rejects_special_files() {
    let dir = tempdir().expect("tempdir");
    let fifo = dir.path().join("pipe");
    std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo");

    let policy = WritePathPolicy::new(vec![dir.path().to_path_buf()]);
    let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
    let err = policy
        .resolve_write_path(&ctx, Path::new("pipe"))
        .await
        .expect_err("fifo");
    assert_eq!(err.code, "unsupported_file_type");
}

fn read_file_tool(dir: &tempfile::TempDir, max_output_bytes: usize) -> ReadFileTool {
    ReadFileTool::new(
        ReadFileConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
        },
        max_output_bytes,
    )
}

fn read_file_ctx(dir: &tempfile::TempDir) -> ToolExecutionContext {
    ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"))
}

fn parse_metadata_line(output: &str) -> serde_json::Value {
    let line = output.lines().next().expect("metadata line");
    assert!(
        line.starts_with(FILE_METADATA_PREFIX),
        "expected metadata prefix, got: {line}"
    );
    let json = line
        .strip_prefix(FILE_METADATA_PREFIX)
        .expect("prefix")
        .trim_start();
    serde_json::from_str(json).expect("metadata json")
}

#[tokio::test]
async fn read_file_default_output_unchanged_without_metadata() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("plain.txt"), "line one\nline two\n").expect("write");

    let tool = read_file_tool(&dir, 4096);
    let ctx = read_file_ctx(&dir);
    let args = serde_json::json!({ "path": "plain.txt", "offset": 2, "limit": 1 });

    let (_record, result) = tool.execute("tc-plain", &args, 5000, &ctx).await;
    assert!(!result.is_error, "{}", result.content);
    assert!(!result.content.starts_with(FILE_METADATA_PREFIX));
    assert_eq!(result.content, "line two");
}

#[tokio::test]
async fn read_file_metadata_includes_sha256() {
    let dir = tempdir().expect("tempdir");
    let body = "alpha\nbeta\n";
    std::fs::write(dir.path().join("hash.txt"), body).expect("write");

    let tool = read_file_tool(&dir, 4096);
    let ctx = read_file_ctx(&dir);
    let args = serde_json::json!({ "path": "hash.txt", "include_metadata": true });

    let (_record, result) = tool.execute("tc-hash", &args, 5000, &ctx).await;
    assert!(!result.is_error, "{}", result.content);
    let meta = parse_metadata_line(&result.content);
    assert_eq!(meta["path"], "hash.txt");
    assert_eq!(meta["sha256"], sha256_hex(body.as_bytes()));
    assert_eq!(meta["size_bytes"], body.len());
}

#[tokio::test]
async fn read_file_metadata_hash_covers_full_file() {
    let dir = tempdir().expect("tempdir");
    let body = "first\nsecond\nthird\n";
    std::fs::write(dir.path().join("slice.txt"), body).expect("write");

    let tool = read_file_tool(&dir, 4096);
    let ctx = read_file_ctx(&dir);
    let args = serde_json::json!({
        "path": "slice.txt",
        "offset": 2,
        "limit": 1,
        "include_metadata": true
    });

    let (_record, result) = tool.execute("tc-slice", &args, 5000, &ctx).await;
    assert!(!result.is_error, "{}", result.content);
    let meta = parse_metadata_line(&result.content);
    assert_eq!(meta["sha256"], sha256_hex(body.as_bytes()));
    let body_only = result
        .content
        .split_once('\n')
        .map(|(_, tail)| tail)
        .expect("body");
    assert_eq!(body_only, "second");
}

#[tokio::test]
async fn read_file_metadata_reports_line_ending() {
    let dir = tempdir().expect("tempdir");
    let cases = [
        ("lf.txt", "a\nb\n", "lf"),
        ("crlf.txt", "a\r\nb\r\n", "crlf"),
        ("none.txt", "plain", "none"),
        ("mixed.txt", "a\nb\r\nc", "mixed"),
    ];

    let tool = read_file_tool(&dir, 4096);
    let ctx = read_file_ctx(&dir);

    for (name, body, expected) in cases {
        std::fs::write(dir.path().join(name), body).expect("write");
        let args = serde_json::json!({ "path": name, "include_metadata": true });
        let (_record, result) = tool.execute("tc-ending", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{name}: {}", result.content);
        let meta = parse_metadata_line(&result.content);
        assert_eq!(meta["line_ending"], expected, "{name}");
    }
}

#[tokio::test]
async fn read_file_metadata_survives_output_truncate() {
    let dir = tempdir().expect("tempdir");
    let body = "x".repeat(400);
    std::fs::write(dir.path().join("big.txt"), format!("{body}\n")).expect("write");

    let tool = read_file_tool(&dir, 300);
    let ctx = read_file_ctx(&dir);
    let args = serde_json::json!({ "path": "big.txt", "include_metadata": true });

    let (_record, result) = tool.execute("tc-trunc", &args, 5000, &ctx).await;
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.starts_with(FILE_METADATA_PREFIX));
    assert!(result.content.contains("[output truncated:"));
    let meta = parse_metadata_line(&result.content);
    assert_eq!(meta["path"], "big.txt");
}

#[test]
fn atomic_write_removes_temp_file_on_success() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("target.txt");
    atomic_write_file(&path, b"hello\n", None).expect("write");
    assert_eq!(std::fs::read(&path).expect("read"), b"hello\n");
    assert!(
        !dir_has_temp_leftovers(dir.path()).expect("scan"),
        "temp file must not remain after successful write"
    );
}

#[test]
fn atomic_write_preserves_original_on_failure() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("target.txt");
    std::fs::write(&path, b"original\n").expect("seed");
    let err = aibe::adapters::outbound::tools::file_atomic::atomic_write_file_fail_before_rename(
        &path,
        b"replacement\n",
        Some(0o644),
    );
    assert!(err.is_err());
    assert_eq!(std::fs::read(&path).expect("read"), b"original\n");
    assert!(
        !dir_has_temp_leftovers(dir.path()).expect("scan"),
        "failed write must not leave temp files"
    );
}

#[test]
fn journal_capacity_exceeded_blocks_write() {
    let dir = tempdir().expect("tempdir");
    let journal = FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: dir.path().join("journal"),
        retention_days: 7,
        max_bytes: 900,
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let payload = vec![b'x'; 300];
    let first = rt.block_on(journal.save_before(JournalSaveRequest {
        tool: "write_file".to_string(),
        target_path: PathBuf::from("/tmp/a.txt"),
        before_state: BeforeState::Present,
        before_bytes: Some(payload.clone()),
        before_sha256: Some(sha256_hex(&payload)),
        after_sha256: sha256_hex(b"after"),
        after_bytes: 5,
        file_mode: Some(0o644),
        operation: FileChangeOperation::Replace,
    }));
    assert!(first.is_ok(), "first journal save should fit");

    let second = rt.block_on(journal.save_before(JournalSaveRequest {
        tool: "write_file".to_string(),
        target_path: PathBuf::from("/tmp/b.txt"),
        before_state: BeforeState::Present,
        before_bytes: Some(payload),
        before_sha256: Some(sha256_hex(b"x")),
        after_sha256: sha256_hex(b"after"),
        after_bytes: 5,
        file_mode: Some(0o644),
        operation: FileChangeOperation::Replace,
    }));
    assert!(matches!(
        second,
        Err(FileChangeJournalError::CapacityExceeded)
    ));
}

#[test]
fn journal_records_absent_before_for_create() {
    let dir = tempdir().expect("tempdir");
    let journal = FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: dir.path().join("journal"),
        retention_days: 7,
        max_bytes: DEFAULT_JOURNAL_MAX_BYTES,
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let entry = rt
        .block_on(journal.save_before(JournalSaveRequest {
            tool: "write_file".to_string(),
            target_path: PathBuf::from("/tmp/new.txt"),
            before_state: BeforeState::Absent,
            before_bytes: None,
            before_sha256: None,
            after_sha256: sha256_hex(b"new"),
            after_bytes: 3,
            file_mode: None,
            operation: FileChangeOperation::Create,
        }))
        .expect("save");
    assert!(!entry.dir.join("before.bin").exists());
    let meta = read_journal_metadata(&entry.dir).expect("metadata");
    assert_eq!(meta["before_state"], "absent");
}

#[test]
fn journal_metadata_excludes_raw_patch() {
    let dir = tempdir().expect("tempdir");
    let journal = FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: dir.path().join("journal"),
        retention_days: 7,
        max_bytes: DEFAULT_JOURNAL_MAX_BYTES,
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let entry = rt
        .block_on(journal.save_before(JournalSaveRequest {
            tool: "apply_patch".to_string(),
            target_path: PathBuf::from("/tmp/file.txt"),
            before_state: BeforeState::Present,
            before_bytes: Some(b"old\n".to_vec()),
            before_sha256: Some(sha256_hex(b"old\n")),
            after_sha256: sha256_hex(b"new\n"),
            after_bytes: 4,
            file_mode: Some(0o644),
            operation: FileChangeOperation::Patch,
        }))
        .expect("save");
    let meta_text = std::fs::read_to_string(entry.dir.join("metadata.json")).expect("read meta");
    let meta: serde_json::Value = serde_json::from_str(&meta_text).expect("parse meta");
    assert!(
        meta.get("raw_patch").is_none(),
        "journal metadata must not contain raw patch fields"
    );
    assert!(!meta_text.contains("+++"));
}

#[test]
fn journal_uses_restricted_permissions() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("journal");
    let journal = FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: root.clone(),
        retention_days: 7,
        max_bytes: DEFAULT_JOURNAL_MAX_BYTES,
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let entry = rt
        .block_on(journal.save_before(JournalSaveRequest {
            tool: "write_file".to_string(),
            target_path: PathBuf::from("/tmp/file.txt"),
            before_state: BeforeState::Present,
            before_bytes: Some(b"before\n".to_vec()),
            before_sha256: Some(sha256_hex(b"before\n")),
            after_sha256: sha256_hex(b"after\n"),
            after_bytes: 6,
            file_mode: Some(0o644),
            operation: FileChangeOperation::Replace,
        }))
        .expect("save");
    assert_eq!(path_mode(&root).expect("root mode"), 0o700);
    assert_eq!(path_mode(&entry.dir).expect("entry mode"), 0o700);
    assert_eq!(
        path_mode(&entry.dir.join("metadata.json")).expect("meta mode"),
        0o600
    );
    assert_eq!(
        path_mode(&entry.dir.join("before.bin")).expect("before mode"),
        0o600
    );
}

#[test]
fn journal_retention_cleanup_removes_expired() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("journal");
    let journal = FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: root.clone(),
        retention_days: 7,
        max_bytes: DEFAULT_JOURNAL_MAX_BYTES,
    });
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let entry = rt
        .block_on(journal.save_before(JournalSaveRequest {
            tool: "write_file".to_string(),
            target_path: PathBuf::from("/tmp/old.txt"),
            before_state: BeforeState::Absent,
            before_bytes: None,
            before_sha256: None,
            after_sha256: sha256_hex(b"x"),
            after_bytes: 1,
            file_mode: None,
            operation: FileChangeOperation::Create,
        }))
        .expect("save");
    set_journal_created_at_for_test(&entry.dir, "2000-01-01T00:00:00Z").expect("backdate");
    rt.block_on(journal.cleanup_expired()).expect("cleanup");
    assert!(
        !entry.dir.exists(),
        "expired journal entry should be removed"
    );
}

#[test]
fn journal_saves_before_state_bytes() {
    let dir = tempdir().expect("tempdir");
    let journal = FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: dir.path().join("journal"),
        retention_days: 7,
        max_bytes: DEFAULT_JOURNAL_MAX_BYTES,
    });
    let before = b"exact bytes\n".to_vec();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let entry = rt
        .block_on(journal.save_before(JournalSaveRequest {
            tool: "write_file".to_string(),
            target_path: PathBuf::from("/tmp/file.txt"),
            before_state: BeforeState::Present,
            before_bytes: Some(before.clone()),
            before_sha256: Some(sha256_hex(&before)),
            after_sha256: sha256_hex(b"after\n"),
            after_bytes: 6,
            file_mode: Some(0o644),
            operation: FileChangeOperation::Replace,
        }))
        .expect("save");
    let saved = std::fs::read(entry.dir.join("before.bin")).expect("before.bin");
    assert_eq!(saved, before);
}

#[test]
fn diff_preview_truncates_at_max_bytes() {
    let before: String = (0..120).map(|i| format!("old-{i}\n")).collect();
    let after: String = (0..120).map(|i| format!("new-{i}\n")).collect();
    let preview = build_unified_diff_preview(
        "big.txt",
        Some(before.as_bytes()),
        after.as_bytes(),
        FileChangeOperation::Replace,
        200,
    );
    assert!(preview.preview_truncated);
    assert!(preview.diff_text.len() <= 200);
    assert_eq!(preview.summary.after_bytes, after.len());
    assert_eq!(preview.summary.before_bytes, before.len());
}

#[test]
fn unified_diff_formats_existing_file() {
    let preview = build_unified_diff_preview(
        "src/main.rs",
        Some(b"old line\n"),
        b"new line\n",
        FileChangeOperation::Replace,
        DEFAULT_MAX_PREVIEW_BYTES,
    );
    assert!(preview
        .diff_text
        .starts_with("--- a/src/main.rs\n+++ b/src/main.rs\n"));
    assert!(preview.diff_text.contains("-old line\n"));
    assert!(preview.diff_text.contains("+new line\n"));
}

#[test]
fn unified_diff_formats_new_file() {
    let preview = build_unified_diff_preview(
        "src/new_file.rs",
        None,
        b"fn main() {}\n",
        FileChangeOperation::Create,
        DEFAULT_MAX_PREVIEW_BYTES,
    );
    assert!(preview
        .diff_text
        .starts_with("--- /dev/null\n+++ b/src/new_file.rs\n"));
    assert!(preview.diff_text.contains("+fn main() {}\n"));
}

struct Phase6ApprovalGate {
    response: Mutex<ToolApprovalGateOutcome>,
    delay: Option<Duration>,
}

impl Phase6ApprovalGate {
    fn fixed(response: ToolApprovalGateOutcome) -> Arc<Self> {
        Arc::new(Self {
            response: Mutex::new(response),
            delay: None,
        })
    }

    fn delayed(response: ToolApprovalGateOutcome, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            response: Mutex::new(response),
            delay: Some(delay),
        })
    }
}

#[async_trait]
impl ToolApprovalGate for Phase6ApprovalGate {
    async fn request_tool_approval(
        &self,
        _tool_call_id: &str,
        _prompt: ToolApprovalPromptRequest,
    ) -> ToolApprovalGateOutcome {
        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
        *self.response.lock().await
    }
}

fn phase6_write_config(approval: FileWriteApprovalMode) -> FileWriteConfig {
    let mut config = FileWriteConfig::default();
    config.approval = approval;
    config
}

fn phase6_service(dir: &Path, config: FileWriteConfig) -> Arc<dyn FileChangeExecutor> {
    let journal = Arc::new(FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: dir.join("journal"),
        retention_days: 7,
        max_bytes: 1_000_000,
    }));
    let store = Arc::new(FilesystemFileChangeStore);
    let path_revalidator = Arc::new(ConfigWritePathRevalidator::from_config(&config));
    Arc::new(FileChangeService::new(
        config,
        journal,
        store,
        path_revalidator,
    ))
}

fn phase6_ctx(dir: &Path, gate: Option<Arc<dyn ToolApprovalGate>>) -> ToolExecutionContext {
    let cwd = ClientCwd::new(dir.to_path_buf()).expect("cwd");
    let mut ctx = ToolExecutionContext::new(cwd).with_turn_id("phase6");
    ctx = ctx.with_capability_policy(StaticCapabilityPolicy::local_full());
    if let Some(gate) = gate {
        ctx = ctx.with_tool_approval_gate(gate);
    }
    ctx
}

fn phase6_tool(dir: &Path, config: FileWriteConfig) -> WriteFileTool {
    let service = phase6_service(dir, config.clone());
    WriteFileTool::new(config, service)
}

fn phase7_tool(dir: &Path, config: FileWriteConfig) -> ApplyPatchTool {
    let service = phase6_service(dir, config.clone());
    ApplyPatchTool::new(config, service)
}

async fn run_apply_patch(
    dir: &Path,
    config: FileWriteConfig,
    gate: Option<Arc<dyn ToolApprovalGate>>,
    args: Value,
) -> (aibe::domain::ExecutedToolCall, aibe::domain::ToolResult) {
    let tool = phase7_tool(dir, config);
    let ctx = phase6_ctx(dir, gate);
    tool.execute("call-1", &args, 30_000, &ctx).await
}

async fn run_write_file(
    dir: &Path,
    config: FileWriteConfig,
    gate: Option<Arc<dyn ToolApprovalGate>>,
    args: Value,
) -> (aibe::domain::ExecutedToolCall, aibe::domain::ToolResult) {
    let tool = phase6_tool(dir, config);
    let ctx = phase6_ctx(dir, gate);
    tool.execute("call-1", &args, 30_000, &ctx).await
}

#[tokio::test]
async fn write_file_detects_stale_file_after_approval_wait() {
    use aibe_protocol::ToolApprovalOrigin;

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("note.txt");
    std::fs::write(&path, "before\n").expect("seed");
    let hash = sha256_hex(b"before\n");
    let gate = Phase6ApprovalGate::delayed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
        Duration::from_millis(200),
    );
    let path_for_task = path.clone();
    let writer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(path_for_task, "external\n").expect("external write");
    });
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate),
        json!({
            "path": "note.txt",
            "mode": "replace",
            "content": "after\n",
            "expected_sha256": hash,
        }),
    )
    .await;
    writer.await.expect("writer");
    assert_eq!(executed.error.as_deref(), Some("stale_file"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "external\n");
}

struct WriteRoundLlm {
    step: Mutex<Option<LlmStepResult>>,
}

impl WriteRoundLlm {
    fn write_file_call() -> Arc<Self> {
        Arc::new(Self {
            step: Mutex::new(Some(LlmStepResult::with_tool_calls(
                "",
                vec![ToolCall {
                    id: "c1".into(),
                    name: "write_file".into(),
                    arguments: json!({
                        "path": "out.txt",
                        "mode": "create",
                        "content": "hello\n",
                    }),
                    provider_extras: None,
                }],
            ))),
        })
    }
}

#[async_trait]
impl LlmProvider for WriteRoundLlm {
    async fn complete(
        &self,
        _messages: &[ChatMessage],
    ) -> Result<ChatMessage, aibe::ports::outbound::LlmError> {
        Err(aibe::ports::outbound::LlmError::Provider("not used".into()))
    }

    async fn complete_with_tools(
        &self,
        _conversation: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, aibe::ports::outbound::LlmError> {
        self.step
            .lock()
            .await
            .take()
            .ok_or_else(|| aibe::ports::outbound::LlmError::Provider("no step".into()))
    }

    async fn complete_with_tools_streaming(
        &self,
        conversation: &[ChatMessage],
        tools: &[ToolDefinition],
        _on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmStepResult, aibe::ports::outbound::LlmError> {
        self.complete_with_tools(conversation, tools).await
    }
}

#[tokio::test]
async fn tool_round_executor_requires_file_write_for_write_tools() {
    let dir = tempdir().expect("tempdir");
    let mut config = phase6_write_config(FileWriteApprovalMode::Always);
    config.approval = FileWriteApprovalMode::Always;
    let tool = Arc::new(phase6_tool(dir.path(), config.clone())) as Arc<dyn ToolExecutor>;
    let registry = Arc::new(DefaultToolRegistry::from_executors([tool]).expect("registry"));
    let llm = WriteRoundLlm::write_file_call();
    let executor = ToolRoundExecutor::new(
        llm,
        registry,
        ToolsConfig::default(),
        Arc::new(NoopLlmCallTracer),
    );
    let cwd = ClientCwd::new(dir.path().to_path_buf()).expect("cwd");
    let ctx = ToolExecutionContext::new(cwd)
        .with_turn_id("round")
        .with_capability_policy(StaticCapabilityPolicy::memory_read_only());
    let outcome = executor
        .run_one_round(
            &[ChatMessage::user("write")],
            &[ToolName::write_file()],
            &[],
            &ctx,
            &[],
            None,
            None,
        )
        .await
        .expect("round");
    match outcome {
        RoundOutcome::Continue { executed, .. } => {
            assert_eq!(executed.len(), 1);
            assert_eq!(executed[0].error.as_deref(), Some("capability_denied"));
        }
        _ => panic!("expected Continue"),
    }
}

#[tokio::test]
async fn write_file_requires_file_write_capability() {
    let dir = tempdir().expect("tempdir");
    let config = phase6_write_config(FileWriteApprovalMode::Always);
    let tool = phase6_tool(dir.path(), config);
    let cwd = ClientCwd::new(dir.path().to_path_buf()).expect("cwd");
    let ctx = ToolExecutionContext::new(cwd)
        .with_capability_policy(StaticCapabilityPolicy::memory_read_only());
    let (executed, result) = tool
        .execute(
            "call-1",
            &json!({
                "path": "out.txt",
                "mode": "create",
                "content": "hello\n",
            }),
            30_000,
            &ctx,
        )
        .await;
    assert!(result.is_error);
    assert_eq!(executed.error.as_deref(), Some("capability_denied"));
}

#[tokio::test]
async fn write_file_create_rejects_missing_parent() {
    let dir = tempdir().expect("tempdir");
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "missing/out.txt",
            "mode": "create",
            "content": "hello\n",
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("parent_not_found"));
}

#[tokio::test]
async fn write_file_create_succeeds() {
    let dir = tempdir().expect("tempdir");
    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "new.txt",
            "mode": "create",
            "content": "hello\n",
        }),
    )
    .await;
    assert!(!result.is_error);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "hello\n"
    );
}

#[tokio::test]
async fn write_file_create_rejects_existing_target() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("exists.txt"), "old\n").expect("seed");
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "exists.txt",
            "mode": "create",
            "content": "new\n",
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("target_exists"));
}

#[tokio::test]
async fn write_file_allows_empty_content() {
    let dir = tempdir().expect("tempdir");
    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "empty.txt",
            "mode": "create",
            "content": "",
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(std::fs::read(dir.path().join("empty.txt")).unwrap(), b"");
}

#[tokio::test]
async fn write_file_replace_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("perm.txt");
    std::fs::write(&path, "before\n").expect("seed");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("chmod");
    let hash = sha256_hex(b"before\n");
    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "perm.txt",
            "mode": "replace",
            "content": "after\n",
            "expected_sha256": hash,
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "after\n");
    assert_eq!(path_mode(&path).expect("mode"), 0o640);
}

#[tokio::test]
async fn write_file_replace_requires_expected_sha256() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "before\n").expect("seed");
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "mode": "replace",
            "content": "after\n",
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("precondition_required"));
}

#[tokio::test]
async fn write_file_replace_succeeds_with_matching_hash() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "before\n").expect("seed");
    let hash = sha256_hex(b"before\n");
    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "mode": "replace",
            "content": "after\n",
            "expected_sha256": hash,
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "after\n"
    );
}

#[tokio::test]
async fn write_file_replace_rejects_stale_hash() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "before\n").expect("seed");
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "mode": "replace",
            "content": "after\n",
            "expected_sha256": "0".repeat(64),
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("stale_file"));
}

#[tokio::test]
async fn apply_patch_single_hunk_succeeds() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "line1\nline2\nline3\n").expect("seed");
    let hash = sha256_hex(b"line1\nline2\nline3\n");
    let patch = "@@ -2,1 +2,1 @@\n-line2\n+LINE2\n";
    let (executed, result) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "line1\nLINE2\nline3\n"
    );
}

#[tokio::test]
async fn apply_patch_multiple_hunks_succeeds() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "a\nb\nc\nd\n").expect("seed");
    let hash = sha256_hex(b"a\nb\nc\nd\n");
    let patch = "@@ -1,1 +1,1 @@\n-a\n+A\n@@ -4,1 +4,1 @@\n-d\n+D\n";
    let (executed, result) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "A\nb\nc\nD\n"
    );
}

#[tokio::test]
async fn apply_patch_rejects_context_mismatch() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "line1\nline2\n").expect("seed");
    let hash = sha256_hex(b"line1\nline2\n");
    let patch = "@@ -2,1 +2,1 @@\n-wrong\n+new\n";
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("patch_conflict"));
}

#[tokio::test]
async fn apply_patch_rejects_overlapping_hunks() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "a\nb\nc\n").expect("seed");
    let hash = sha256_hex(b"a\nb\nc\n");
    let patch = "@@ -1,2 +1,2 @@\n a\n-b\n@@ -2,1 +2,1 @@\n-b\n+B\n";
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("invalid_patch"));
}

#[tokio::test]
async fn apply_patch_rejects_diff_headers() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "line1\n").expect("seed");
    let hash = sha256_hex(b"line1\n");
    let patch = "--- a/note.txt\n+++ b/note.txt\n@@ -1,1 +1,1 @@\n-line1\n+LINE1\n";
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("invalid_patch"));
}

#[tokio::test]
async fn apply_patch_rejects_empty_patch() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "line1\n").expect("seed");
    let hash = sha256_hex(b"line1\n");
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": "",
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("invalid_patch"));
}

#[tokio::test]
async fn apply_patch_no_change_skips_write() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("note.txt");
    std::fs::write(&path, "line1\nline2\n").expect("seed");
    let before = std::fs::read(&path).expect("read");
    let hash = sha256_hex(&before);
    let patch = "@@ -1,2 +1,2 @@\n line1\n line2\n";
    let (executed, result) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(executed.decision.as_deref(), Some("no_change"));
    assert_eq!(std::fs::read(&path).expect("read"), before);
    assert!(
        !dir.path().join("journal").exists(),
        "no journal entry on no_change"
    );
}

#[tokio::test]
async fn apply_patch_preserves_crlf() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("note.txt");
    std::fs::write(&path, "line1\r\nline2\r\n").expect("seed");
    let hash = sha256_hex(b"line1\r\nline2\r\n");
    let patch = "@@ -2,1 +2,1 @@\n-line2\n+LINE2\n";
    let (executed, result) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(std::fs::read(&path).expect("read"), b"line1\r\nLINE2\r\n");
}

#[tokio::test]
async fn apply_patch_rejects_mixed_line_endings() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "a\nb\r\nc\n").expect("seed");
    let hash = sha256_hex(b"a\nb\r\nc\n");
    let patch = "@@ -2,1 +2,1 @@\n-b\n+B\n";
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("unsupported_line_endings"));
}

#[tokio::test]
async fn apply_patch_enforces_patch_size_limit() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "line1\n").expect("seed");
    let hash = sha256_hex(b"line1\n");
    let mut config = phase6_write_config(FileWriteApprovalMode::Always);
    config.max_patch_bytes = 10;
    let patch = "@@ -1,1 +1,1 @@\n-line1\n+this patch is too long\n";
    let (executed, _) = run_apply_patch(
        dir.path(),
        config,
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": patch,
        }),
    )
    .await;
    assert_eq!(executed.error.as_deref(), Some("input_too_large"));
}

#[tokio::test]
async fn apply_patch_detects_stale_file_after_approval_wait() {
    use aibe_protocol::ToolApprovalOrigin;

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("note.txt");
    std::fs::write(&path, "before\n").expect("seed");
    let hash = sha256_hex(b"before\n");
    let gate = Phase6ApprovalGate::delayed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
        Duration::from_millis(200),
    );
    let path_for_task = path.clone();
    let writer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(path_for_task, "external\n").expect("external write");
    });
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate),
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": "@@ -1,1 +1,1 @@\n-before\n+after\n",
        }),
    )
    .await;
    writer.await.expect("writer");
    assert_eq!(executed.error.as_deref(), Some("stale_file"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "external\n");
}

#[test]
fn mixed_shell_and_write_approval_in_one_turn() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::thread;

    use aibe_client::{
        agent_turn_on_stream_with_callbacks, AgentTurnCallbacks, ShellExecApprovalDecision,
        ShellExecApprovalPrompt, ToolApprovalDecision, ToolApprovalPrompt,
    };
    use aibe_protocol::{
        AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessage, ProtocolMessageOut,
        ShellExecApprovalOrigin, ToolApprovalOrigin, ToolRiskClass, WRITE_FILE,
    };

    let (client, server) = UnixStream::pair().expect("pair");
    let handle = thread::spawn(move || {
        let mut server = server;
        let mut reader = BufReader::new(server.try_clone().expect("clone"));
        let mut line = String::new();
        reader.read_line(&mut line).expect("read request");

        let shell_prompt = ClientResponse::ShellExecApprovalPrompt {
            id: "shell-prompt".into(),
            turn_id: "turn-mixed".into(),
            tool_call_id: "call-shell".into(),
            command: "echo".into(),
            args: vec!["hi".into()],
        };
        writeln!(
            server,
            "{}",
            serde_json::to_string(&shell_prompt).expect("shell prompt")
        )
        .expect("write shell prompt");
        server.flush().expect("flush");

        line.clear();
        reader.read_line(&mut line).expect("read shell approval");
        let shell_approval: ClientRequest =
            serde_json::from_str(line.trim()).expect("parse shell approval");
        assert!(matches!(
            shell_approval,
            ClientRequest::ShellExecApproval {
                approved: true,
                approval_origin: ShellExecApprovalOrigin::UiYes,
                ..
            }
        ));

        let tool_prompt = ClientResponse::ToolApprovalPrompt {
            id: "tool-prompt".into(),
            turn_id: "turn-mixed".into(),
            tool_call_id: "call-write".into(),
            tool_name: WRITE_FILE.into(),
            risk_class: ToolRiskClass::WriteLike,
            summary: "create demo.txt (+1 -0, 0 -> 5 bytes)".into(),
            paths: vec!["demo.txt".into()],
            preview: "+hello\n".into(),
            preview_truncated: false,
        };
        writeln!(
            server,
            "{}",
            serde_json::to_string(&tool_prompt).expect("tool prompt")
        )
        .expect("write tool prompt");
        server.flush().expect("flush");

        line.clear();
        reader.read_line(&mut line).expect("read tool approval");
        let tool_approval: ClientRequest =
            serde_json::from_str(line.trim()).expect("parse tool approval");
        assert!(matches!(
            tool_approval,
            ClientRequest::ToolApproval {
                approved: false,
                approval_origin: ToolApprovalOrigin::UiNo,
                ..
            }
        ));

        let final_resp = ClientResponse::AgentTurnResult {
            id: "turn-mixed".into(),
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "mixed approvals handled".into(),
            },
            tool_calls: vec![],
        };
        writeln!(
            server,
            "{}",
            serde_json::to_string(&final_resp).expect("final")
        )
        .expect("write final");
        server.flush().expect("flush final");
    });

    let mut shell_seen = false;
    let mut tool_seen = false;
    let resp = agent_turn_on_stream_with_callbacks(
        client,
        ClientRequest::AgentTurn {
            id: "turn-mixed".into(),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "mixed".into(),
            }],
            tools: vec!["shell_exec".into(), WRITE_FILE.into()],
            client_tools: vec![],
            context: Default::default(),
            llm_profile: None,
        },
        AgentTurnCallbacks::new(
            |prompt: ShellExecApprovalPrompt| {
                shell_seen = true;
                assert_eq!(prompt.command, "echo");
                ShellExecApprovalDecision {
                    approved: true,
                    approval_origin: ShellExecApprovalOrigin::UiYes,
                }
            },
            |prompt: ToolApprovalPrompt| {
                tool_seen = true;
                assert_eq!(prompt.tool_name, WRITE_FILE);
                ToolApprovalDecision::Denied(ToolApprovalOrigin::UiNo)
            },
        ),
    )
    .expect("agent turn");

    handle.join().expect("server");
    assert!(shell_seen);
    assert!(tool_seen);
    match resp {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => assert_eq!(assistant_message.content, "mixed approvals handled"),
        other => panic!("expected agent_turn_result, got {other:?}"),
    }
}

#[tokio::test]
async fn acceptance_write_file_create_flow() {
    use aibe_protocol::ClientResponse;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::sync::Mutex;

    use aibe::adapters::inbound::connection_approval::ConnectionApprovalGate;

    let dir = tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("src")).expect("mkdir src");
    let socket_path = dir.path().join("create.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind");
    let workdir = dir.path().to_path_buf();
    let target = workdir.join("src").join("example.rs");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let lines = Arc::new(Mutex::new(BufReader::new(reader).lines()));
        let gate = Arc::new(ConnectionApprovalGate::new(
            "turn-create".into(),
            Arc::clone(&writer),
            Arc::clone(&lines),
            None,
            None,
        ));
        let config = phase6_write_config(FileWriteApprovalMode::Ask);
        let tool = phase6_tool(&workdir, config);
        let ctx = phase6_ctx(&workdir, Some(gate));
        tool.execute(
            "call-create",
            &json!({
                "path": "src/example.rs",
                "mode": "create",
                "content": "fn main() {}\n",
            }),
            30_000,
            &ctx,
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let prompt_line = read_until_tool_approval_prompt(&mut lines).await;
    let prompt: ClientResponse = serde_json::from_str(prompt_line.trim()).expect("prompt json");
    let ClientResponse::ToolApprovalPrompt {
        id,
        turn_id,
        tool_call_id,
        tool_name,
        preview,
        ..
    } = prompt
    else {
        panic!("expected tool_approval_prompt, got {prompt_line}");
    };
    assert_eq!(turn_id, "turn-create");
    assert_eq!(tool_call_id, "call-create");
    assert_eq!(tool_name, "write_file");
    assert!(preview.contains("fn main()"));

    let approval = json!({
        "type": "tool_approval",
        "id": id,
        "turn_id": turn_id,
        "tool_call_id": tool_call_id,
        "approved": true,
        "approval_origin": "ui_yes"
    });
    writer
        .write_all(format!("{approval}\n").as_bytes())
        .await
        .expect("write approval");
    writer.flush().await.expect("flush approval");

    let (executed, result) = server.await.expect("server");
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    assert_eq!(
        std::fs::read_to_string(&target).expect("read created file"),
        "fn main() {}\n"
    );
}

#[tokio::test]
async fn acceptance_apply_patch_flow() {
    use aibe_protocol::ClientResponse;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::sync::Mutex;

    use aibe::adapters::inbound::connection_approval::ConnectionApprovalGate;

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("handler.rs");
    std::fs::write(&path, "fn handle() {\n    todo!()\n}\n").expect("seed");
    let hash = sha256_hex(b"fn handle() {\n    todo!()\n}\n");
    let socket_path = dir.path().join("patch.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind");
    let workdir = dir.path().to_path_buf();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let lines = Arc::new(Mutex::new(BufReader::new(reader).lines()));
        let gate = Arc::new(ConnectionApprovalGate::new(
            "turn-patch".into(),
            Arc::clone(&writer),
            Arc::clone(&lines),
            None,
            None,
        ));
        let config = phase6_write_config(FileWriteApprovalMode::Ask);
        let tool = phase7_tool(&workdir, config);
        let ctx = phase6_ctx(&workdir, Some(gate));
        tool.execute(
            "call-patch",
            &json!({
                "path": "handler.rs",
                "expected_sha256": hash,
                "patch": "@@ -2,1 +2,1 @@\n-    todo!()\n+    Ok(())\n",
            }),
            30_000,
            &ctx,
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let prompt_line = read_until_tool_approval_prompt(&mut lines).await;
    let prompt: ClientResponse = serde_json::from_str(prompt_line.trim()).expect("prompt json");
    let ClientResponse::ToolApprovalPrompt {
        id,
        turn_id,
        tool_call_id,
        tool_name,
        preview,
        ..
    } = prompt
    else {
        panic!("expected tool_approval_prompt, got {prompt_line}");
    };
    assert_eq!(turn_id, "turn-patch");
    assert_eq!(tool_call_id, "call-patch");
    assert_eq!(tool_name, "apply_patch");
    assert!(preview.contains("-    todo!()"));
    assert!(preview.contains("+    Ok(())"));

    let approval = json!({
        "type": "tool_approval",
        "id": id,
        "turn_id": turn_id,
        "tool_call_id": tool_call_id,
        "approved": true,
        "approval_origin": "ui_yes"
    });
    writer
        .write_all(format!("{approval}\n").as_bytes())
        .await
        .expect("write approval");
    writer.flush().await.expect("flush approval");

    let (executed, result) = server.await.expect("server");
    assert!(!result.is_error, "{}", result.content);
    assert!(executed
        .output
        .as_deref()
        .unwrap_or("")
        .contains("change_id="));
    let updated = std::fs::read_to_string(&path).expect("read patched file");
    assert!(updated.contains("Ok(())"));
    assert!(!updated.contains("todo!()"));
}

#[tokio::test]
async fn write_tools_audit_uses_fixed_approval_source() {
    use aibe_protocol::ToolApprovalOrigin;

    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("yes.txt"), "before\n").expect("seed");
    let yes_hash = sha256_hex(b"before\n");

    let gate_yes =
        Phase6ApprovalGate::fixed(ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes));
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate_yes),
        json!({
            "path": "yes.txt",
            "mode": "replace",
            "expected_sha256": yes_hash,
            "content": "after\n",
        }),
    )
    .await;
    assert_eq!(
        executed.approval_source.as_deref(),
        Some("file_write_approval=ask;ui=y")
    );

    std::fs::write(dir.path().join("no.txt"), "keep\n").expect("seed");
    let no_hash = sha256_hex(b"keep\n");
    let gate_no =
        Phase6ApprovalGate::fixed(ToolApprovalGateOutcome::Denied(ToolApprovalOrigin::UiNo));
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate_no),
        json!({
            "path": "no.txt",
            "mode": "replace",
            "expected_sha256": no_hash,
            "content": "new\n",
        }),
    )
    .await;
    assert_eq!(
        executed.approval_source.as_deref(),
        Some("file_write_approval=ask;ui=n")
    );

    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        None,
        json!({
            "path": "new2.txt",
            "mode": "create",
            "content": "hello\n",
        }),
    )
    .await;
    assert_eq!(
        executed.approval_source.as_deref(),
        Some("file_write_approval=ask")
    );

    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "auto.txt",
            "mode": "create",
            "content": "auto\n",
        }),
    )
    .await;
    assert_eq!(
        executed.approval_source.as_deref(),
        Some("file_write_approval=always")
    );
}

#[tokio::test]
async fn write_tools_audit_decision_matrix() {
    use aibe_protocol::ToolApprovalOrigin;

    let dir = tempdir().expect("tempdir");

    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "created.txt",
            "mode": "create",
            "content": "hello\n",
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(executed.decision.as_deref(), Some("executed"));

    std::fs::write(dir.path().join("deny.txt"), "keep\n").expect("seed");
    let hash = sha256_hex(b"keep\n");
    let gate_no =
        Phase6ApprovalGate::fixed(ToolApprovalGateOutcome::Denied(ToolApprovalOrigin::UiNo));
    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate_no),
        json!({
            "path": "deny.txt",
            "mode": "replace",
            "expected_sha256": hash,
            "content": "new\n",
        }),
    )
    .await;
    assert!(result.is_error);
    assert_eq!(executed.decision.as_deref(), Some("rejected_by_user"));

    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        None,
        json!({
            "path": "unavail.txt",
            "mode": "create",
            "content": "x\n",
        }),
    )
    .await;
    assert!(result.is_error);
    assert_eq!(executed.decision.as_deref(), Some("approval_unavailable"));

    let path = dir.path().join("stale.txt");
    std::fs::write(&path, "before\n").expect("seed");
    let stale_hash = sha256_hex(b"before\n");
    let gate_yes = Phase6ApprovalGate::delayed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
        Duration::from_millis(200),
    );
    let path_for_task = path.clone();
    let writer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(path_for_task, "external\n").expect("external write");
    });
    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate_yes),
        json!({
            "path": "stale.txt",
            "mode": "replace",
            "expected_sha256": stale_hash,
            "content": "after\n",
        }),
    )
    .await;
    writer.await.expect("writer");
    assert!(result.is_error);
    assert_eq!(executed.decision.as_deref(), Some("rejected_or_failed"));

    std::fs::write(dir.path().join("note.txt"), "line1\nline2\n").expect("seed");
    let before = std::fs::read(dir.path().join("note.txt")).expect("read");
    let note_hash = sha256_hex(&before);
    let patch = "@@ -1,2 +1,2 @@\n line1\n line2\n";
    let (executed, result) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": note_hash,
            "patch": patch,
        }),
    )
    .await;
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(executed.decision.as_deref(), Some("no_change"));
}

#[tokio::test]
async fn write_tools_audit_uses_write_like_risk_class() {
    use aibe::domain::ToolRiskClass;

    let dir = tempdir().expect("tempdir");
    let (executed, _) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "new.txt",
            "mode": "create",
            "content": "hello\n",
        }),
    )
    .await;
    assert_eq!(executed.risk_class, Some(ToolRiskClass::WriteLike));

    std::fs::write(dir.path().join("note.txt"), "line1\n").expect("seed");
    let hash = sha256_hex(b"line1\n");
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "expected_sha256": hash,
            "patch": "@@ -1,1 +1,1 @@\n-line1\n+LINE1\n",
        }),
    )
    .await;
    assert_eq!(executed.risk_class, Some(ToolRiskClass::WriteLike));
}

#[tokio::test]
async fn disconnect_during_write_approval_writes_nothing() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::UnixStream;
    use tokio::sync::Mutex;

    use aibe::adapters::inbound::connection_approval::ConnectionApprovalGate;

    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("disconnect.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind");
    let workdir = dir.path().to_path_buf();
    let target = workdir.join("pending.txt");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let lines = Arc::new(Mutex::new(BufReader::new(reader).lines()));
        let gate = Arc::new(ConnectionApprovalGate::new(
            "turn-disconnect".into(),
            Arc::clone(&writer),
            Arc::clone(&lines),
            None,
            None,
        ));
        let config = phase6_write_config(FileWriteApprovalMode::Ask);
        let tool = phase6_tool(&workdir, config);
        let ctx = phase6_ctx(&workdir, Some(gate));
        let (executed, result) = tool
            .execute(
                "call-disconnect",
                &json!({
                    "path": "pending.txt",
                    "mode": "create",
                    "content": "should not land\n",
                }),
                30_000,
                &ctx,
            )
            .await;
        assert!(result.is_error);
        assert_eq!(executed.error.as_deref(), Some("approval_unavailable"));
        assert!(!target.exists());
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await.expect("read") {
        if line.contains(r#""type":"tool_approval_prompt""#) {
            break;
        }
    }
    drop(lines);
    drop(writer);

    server.await.expect("server");
}

#[tokio::test]
async fn shell_exec_approval_regression_unchanged() {
    use async_trait::async_trait;

    use aibe::adapters::outbound::tools::{ConfigAllowlistPolicy, ShellExecTool};
    use aibe::domain::{ClientCwd, ToolApprovalState, ToolRiskClass, SHELL_EXEC};
    use aibe::ports::outbound::{ShellExecApprovalGate, ShellExecApprovalMode, ShellExecConfig};
    use aibe_client::ShellExecApprovalDecision;
    use aibe_protocol::ShellExecApprovalOrigin;

    struct YesGate;

    #[async_trait]
    impl ShellExecApprovalGate for YesGate {
        async fn request_shell_exec_approval(
            &self,
            _tool_call_id: &str,
            _command: &str,
            _args: &[String],
        ) -> Option<ShellExecApprovalDecision> {
            Some(ShellExecApprovalDecision {
                approved: true,
                approval_origin: ShellExecApprovalOrigin::UiYes,
            })
        }
    }

    let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
        enabled: true,
        allowed_commands: vec!["echo".into()],
        approval: ShellExecApprovalMode::Ask,
        ..Default::default()
    }));
    let tool = ShellExecTool::new(policy, 4096, Vec::new());
    let cwd = ClientCwd::new(std::env::current_dir().expect("cwd")).expect("cwd");
    let ctx = ToolExecutionContext::new(cwd)
        .with_turn_id("regression")
        .with_capability_policy(StaticCapabilityPolicy::local_full())
        .with_approval_gate(Arc::new(YesGate));
    let (record, result) = tool
        .execute(
            "tc-reg",
            &json!({"command": "echo", "args": ["ok"]}),
            5000,
            &ctx,
        )
        .await;
    assert!(!result.is_error, "{}", result.content);
    assert_eq!(record.risk_class, Some(ToolRiskClass::DangerousShell));
    assert_eq!(
        record.approval_state,
        Some(ToolApprovalState::ExplicitClientOptIn)
    );
    assert_eq!(record.decision.as_deref(), Some("executed"));
    assert_eq!(
        record.approval_source.as_deref(),
        Some("shell_exec_approval=ask;ui=y")
    );
    assert_eq!(record.name, SHELL_EXEC);
}

async fn read_until_tool_approval_prompt(
    lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> String {
    loop {
        let line = lines.next_line().await.expect("read line").expect("line");
        if line.contains(r#""type":"tool_approval_prompt""#) {
            return line;
        }
    }
}

#[tokio::test]
async fn write_file_error_does_not_audit_raw_content() {
    let dir = tempdir().expect("tempdir");
    let (executed, result) = run_write_file(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "secret.txt",
            "mode": "invalid-mode",
            "content": "API_KEY=super-secret",
        }),
    )
    .await;
    assert!(result.is_error);
    let args = executed.arguments;
    assert_eq!(args["content_bytes"], 20);
    assert!(args.get("content").is_none());
    assert_eq!(args["path"], "secret.txt");
}

#[tokio::test]
async fn apply_patch_error_does_not_audit_raw_patch() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "line\n").expect("seed");
    let (executed, _) = run_apply_patch(
        dir.path(),
        phase6_write_config(FileWriteApprovalMode::Always),
        None,
        json!({
            "path": "note.txt",
            "patch": "not a valid patch",
            "expected_sha256": sha256_hex(b"line\n"),
        }),
    )
    .await;
    let args = executed.arguments;
    assert!(args.get("patch").is_none());
    assert_eq!(args["patch_bytes"], 17);
}

#[tokio::test]
async fn write_tool_capability_rejection_sanitizes_arguments() {
    let dir = tempdir().expect("tempdir");
    let config = phase6_write_config(FileWriteApprovalMode::Always);
    let tool = Arc::new(phase6_tool(dir.path(), config)) as Arc<dyn ToolExecutor>;
    let registry = Arc::new(DefaultToolRegistry::from_executors([tool]).expect("registry"));
    let llm = WriteRoundLlm::write_file_call();
    let executor = ToolRoundExecutor::new(
        llm,
        registry,
        ToolsConfig::default(),
        Arc::new(NoopLlmCallTracer),
    );
    let cwd = ClientCwd::new(dir.path().to_path_buf()).expect("cwd");
    let ctx = ToolExecutionContext::new(cwd)
        .with_turn_id("cap-sanitize")
        .with_capability_policy(StaticCapabilityPolicy::memory_read_only());
    let outcome = executor
        .run_one_round(
            &[ChatMessage::user("write")],
            &[ToolName::write_file()],
            &[],
            &ctx,
            &[],
            None,
            None,
        )
        .await
        .expect("round");
    match outcome {
        RoundOutcome::Continue { executed, .. } => {
            assert_eq!(executed.len(), 1);
            assert_eq!(executed[0].error.as_deref(), Some("capability_denied"));
            assert!(executed[0].arguments.get("content").is_none());
            assert_eq!(executed[0].arguments["content_bytes"], 6);
        }
        _ => panic!("expected Continue"),
    }
}

#[tokio::test]
async fn replace_rejects_oversized_existing_file_before_read() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("big.txt");
    let mut config = phase6_write_config(FileWriteApprovalMode::Always);
    config.max_file_bytes = 16;
    std::fs::write(&path, vec![b'x'; 32]).expect("seed");
    let hash = sha256_hex(&vec![b'x'; 32]);
    let (executed, result) = run_write_file(
        dir.path(),
        config,
        None,
        json!({
            "path": "big.txt",
            "mode": "replace",
            "expected_sha256": hash,
            "content": "small\n",
        }),
    )
    .await;
    assert!(result.is_error);
    assert_eq!(executed.error.as_deref(), Some("file_too_large"));
}

#[cfg(unix)]
#[tokio::test]
async fn write_revalidates_parent_symlink_after_approval() {
    use std::os::unix::fs::symlink;

    use aibe_protocol::ToolApprovalOrigin;

    let base = tempdir().expect("base");
    let outside = tempdir().expect("outside");
    let project = base.path().join("project");
    std::fs::create_dir_all(&project).expect("mkdir");
    std::fs::write(project.join("note.txt"), "before\n").expect("seed");
    let hash = sha256_hex(b"before\n");

    let gate = Phase6ApprovalGate::delayed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
        Duration::from_millis(200),
    );
    let base_for_task = base.path().to_path_buf();
    let outside_path = outside.path().to_path_buf();
    let swapper = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let project = base_for_task.join("project");
        std::fs::remove_dir_all(&project).expect("remove project");
        symlink(&outside_path, &project).expect("symlink project");
        std::fs::write(outside_path.join("note.txt"), "before\n").expect("same hash outside");
    });

    let (executed, _) = run_write_file(
        base.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate),
        json!({
            "path": "project/note.txt",
            "mode": "replace",
            "content": "after\n",
            "expected_sha256": hash,
        }),
    )
    .await;
    swapper.await.expect("swapper");
    assert_eq!(executed.error.as_deref(), Some("stale_file"));
}

#[cfg(unix)]
#[tokio::test]
async fn write_revalidates_target_symlink_after_approval() {
    use std::os::unix::fs::symlink;

    use aibe_protocol::ToolApprovalOrigin;

    let base = tempdir().expect("base");
    let outside = tempdir().expect("outside");
    std::fs::write(base.path().join("note.txt"), "before\n").expect("seed");
    let hash = sha256_hex(b"before\n");

    let gate = Phase6ApprovalGate::delayed(
        ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes),
        Duration::from_millis(200),
    );
    let target = base.path().join("note.txt");
    let outside_file = outside.path().join("note.txt");
    let swapper = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::remove_file(&target).expect("remove");
        std::fs::write(&outside_file, "before\n").expect("same bytes");
        symlink(&outside_file, &target).expect("symlink target");
    });

    let (executed, _) = run_write_file(
        base.path(),
        phase6_write_config(FileWriteApprovalMode::Ask),
        Some(gate),
        json!({
            "path": "note.txt",
            "mode": "replace",
            "content": "after\n",
            "expected_sha256": hash,
        }),
    )
    .await;
    swapper.await.expect("swapper");
    assert_eq!(executed.error.as_deref(), Some("stale_file"));
}

struct FailingCommitStore;

#[async_trait]
impl FileChangeStore for FailingCommitStore {
    async fn commit_atomic(
        &self,
        _path: &Path,
        _content: &[u8],
        _preserve_mode: Option<u32>,
    ) -> Result<(), FileChangeStoreError> {
        Err(FileChangeStoreError)
    }

    async fn is_regular_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    async fn read_file_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, FileChangeStoreError> {
        if !path.is_file() {
            return Ok(None);
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|_| FileChangeStoreError)
    }

    async fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    async fn file_byte_len(&self, path: &Path) -> Result<Option<u64>, FileChangeStoreError> {
        if !path.is_file() {
            return Ok(None);
        }
        std::fs::metadata(path)
            .map(|meta| Some(meta.len()))
            .map_err(|_| FileChangeStoreError)
    }
}

#[tokio::test]
async fn journal_is_not_committed_when_atomic_write_fails() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "before\n").expect("seed");
    let journal = Arc::new(FilesystemFileChangeJournal::new(FileChangeJournalConfig {
        root: dir.path().join("journal"),
        retention_days: 7,
        max_bytes: 1_000_000,
    }));
    let service = Arc::new(FileChangeService::new(
        phase6_write_config(FileWriteApprovalMode::Always),
        journal,
        Arc::new(FailingCommitStore),
        Arc::new(ConfigWritePathRevalidator::from_config(
            &phase6_write_config(FileWriteApprovalMode::Always),
        )),
    ));
    let tool = WriteFileTool::new(phase6_write_config(FileWriteApprovalMode::Always), service);
    let ctx = phase6_ctx(dir.path(), None);
    let hash = sha256_hex(b"before\n");
    let (executed, _) = tool
        .execute(
            "call-journal",
            &json!({
                "path": "note.txt",
                "mode": "replace",
                "content": "after\n",
                "expected_sha256": hash,
            }),
            30_000,
            &ctx,
        )
        .await;
    assert_eq!(executed.error.as_deref(), Some("write_failed"));

    let dates = std::fs::read_dir(dir.path().join("journal"))
        .expect("journal root")
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(dates.len(), 1);
    let changes = std::fs::read_dir(dates[0].path())
        .expect("date dir")
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(changes.len(), 1);
    let meta = read_journal_metadata(&changes[0].path()).expect("metadata");
    assert_eq!(meta["status"], "commit_failed");
}
