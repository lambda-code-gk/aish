// RED stubs for 0054 Safe File Write Tools.
// Removed from #[ignore] when the corresponding phase lands.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::tools::{
    atomic_write_file, build_unified_diff_preview, dir_has_temp_leftovers, DefaultToolRegistry,
    ReadFileTool, ReadPathPolicy, WriteFileTool, WritePathPolicy, FILE_METADATA_PREFIX,
};
use aibe::adapters::outbound::{
    path_mode, read_journal_metadata, set_journal_created_at_for_test, FileChangeJournalConfig,
    FilesystemFileChangeJournal, FilesystemFileChangeStore, StaticCapabilityPolicy,
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
    FileChangeJournal, FileChangeJournalError, FileWriteApprovalMode, FileWriteConfig,
    JournalSaveRequest, LlmProvider, NoopLlmCallTracer, ReadFileConfig, ToolApprovalGate,
    ToolApprovalGateOutcome, ToolApprovalPromptRequest, ToolDefinition, ToolExecutionContext,
    ToolExecutor, ToolsConfig,
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
        raw_patch: None,
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
        raw_patch: None,
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
            raw_patch: None,
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
    let patch = "+++ b/file\n+secret patch body\n".to_string();
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
            raw_patch: Some(patch.clone()),
        }))
        .expect("save");
    let meta_text = std::fs::read_to_string(entry.dir.join("metadata.json")).expect("read meta");
    assert!(
        !meta_text.contains("secret patch body"),
        "raw patch must not be persisted in metadata"
    );
    assert!(!meta_text.contains("+++ b/file"));
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
            raw_patch: None,
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
            raw_patch: None,
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
            raw_patch: None,
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
    #[allow(dead_code)]
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
    Arc::new(FileChangeService::new(config, journal, store))
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

#[test]
#[ignore = "0054 phase 7: apply_patch_context_mismatch"]
fn apply_patch_rejects_context_mismatch() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_crlf"]
fn apply_patch_preserves_crlf() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_empty_invalid"]
fn apply_patch_rejects_empty_patch() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_mixed_line_endings"]
fn apply_patch_rejects_mixed_line_endings() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_multiple_hunks"]
fn apply_patch_multiple_hunks_succeeds() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_no_change"]
fn apply_patch_no_change_skips_write() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_overlapping_hunks"]
fn apply_patch_rejects_overlapping_hunks() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_rejects_headers"]
fn apply_patch_rejects_diff_headers() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_single_hunk"]
fn apply_patch_single_hunk_succeeds() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: apply_patch_size_limit"]
fn apply_patch_enforces_patch_size_limit() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 7: race_stale_apply_patch"]
fn apply_patch_detects_stale_file_after_approval_wait() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 8: shell_and_write_approval_mixed"]
fn mixed_shell_and_write_approval_in_one_turn() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 9: acceptance_create_scenario"]
fn acceptance_write_file_create_flow() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 9: acceptance_patch_scenario"]
fn acceptance_apply_patch_flow() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 9: audit_approval_source_vocabulary"]
fn write_tools_audit_uses_fixed_approval_source() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 9: audit_decision_matrix"]
fn write_tools_audit_decision_matrix() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 9: audit_write_like_risk_class"]
fn write_tools_audit_uses_write_like_risk_class() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 9: disconnect_during_approval"]
fn disconnect_during_write_approval_writes_nothing() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 9: shell_exec_regression"]
fn shell_exec_approval_regression_unchanged() {
    panic!("0054 not implemented");
}
