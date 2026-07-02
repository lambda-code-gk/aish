// RED stubs for 0054 Safe File Write Tools.
// Removed from #[ignore] when the corresponding phase lands.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aibe::adapters::outbound::tools::{
    DefaultToolRegistry, ReadFileTool, ReadPathPolicy, WritePathPolicy, FILE_METADATA_PREFIX,
};
use aibe::domain::{
    check_file_size, detect_line_ending, sha256_hex, validate_utf8_bytes, Capability, ClientCwd,
    FileTextError, LineEnding, ToolName,
};
use aibe::ports::outbound::{
    FileWriteApprovalMode, FileWriteConfig, ReadFileConfig, ToolExecutionContext, ToolExecutor,
    ToolsConfig, DEFAULT_JOURNAL_MAX_BYTES, DEFAULT_JOURNAL_RETENTION_DAYS,
    DEFAULT_MAX_FILE_WRITE_BYTES, DEFAULT_MAX_PATCH_BYTES, DEFAULT_MAX_PREVIEW_BYTES,
};
use async_trait::async_trait;
use serde_json::Value;
use tempfile::tempdir;

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
#[ignore = "0054 phase 4: atomic_write_no_temp_leftover"]
fn atomic_write_removes_temp_file_on_success() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: atomic_write_preserves_original"]
fn atomic_write_preserves_original_on_failure() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: journal_capacity_exceeded"]
fn journal_capacity_exceeded_blocks_write() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: journal_create_absent"]
fn journal_records_absent_before_for_create() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: journal_no_raw_patch"]
fn journal_metadata_excludes_raw_patch() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: journal_permissions"]
fn journal_uses_restricted_permissions() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: journal_retention_cleanup"]
fn journal_retention_cleanup_removes_expired() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: journal_saves_before_bytes"]
fn journal_saves_before_state_bytes() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: preview_truncation"]
fn diff_preview_truncates_at_max_bytes() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: unified_diff_existing_file"]
fn unified_diff_formats_existing_file() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 4: unified_diff_new_file"]
fn unified_diff_formats_new_file() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: approval_gate_missing"]
fn file_change_missing_gate_returns_unavailable() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: cancel_during_approval"]
fn file_change_cancel_during_approval_writes_nothing() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: fake_gate_no_denies"]
fn file_change_fake_gate_no_leaves_file_unchanged() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: fake_gate_yes_commits"]
fn file_change_fake_gate_yes_commits() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: file_change_prepare_no_write"]
fn file_change_prepare_does_not_mutate_file() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: policy_always_skips_prompt"]
fn file_write_always_mode_skips_prompt() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: policy_never_denies"]
fn file_write_never_mode_denies_execution() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: revalidate_stale_file"]
fn file_change_revalidate_detects_stale_file() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: sanitized_arguments"]
fn file_change_sanitizes_executed_tool_arguments() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: tool_approval_wire_roundtrip"]
fn tool_approval_wire_roundtrip() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 5: tool_disabled_when_config_off"]
fn file_write_disabled_returns_tool_disabled() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: race_stale_write_file"]
fn write_file_detects_stale_file_after_approval_wait() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: tool_round_capability_gate"]
fn tool_round_executor_requires_file_write_for_write_tools() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_capability_gate"]
fn write_file_requires_file_write_capability() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_create_parent_missing"]
fn write_file_create_rejects_missing_parent() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_create_success"]
fn write_file_create_succeeds() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_create_target_exists"]
fn write_file_create_rejects_existing_target() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_empty_content"]
fn write_file_allows_empty_content() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_preserves_permissions"]
fn write_file_replace_preserves_permissions() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_replace_requires_hash"]
fn write_file_replace_requires_expected_sha256() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_replace_success"]
fn write_file_replace_succeeds_with_matching_hash() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 6: write_file_stale_hash"]
fn write_file_replace_rejects_stale_hash() {
    panic!("0054 not implemented");
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
