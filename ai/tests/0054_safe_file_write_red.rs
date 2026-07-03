// RED stubs for 0054 Safe File Write Tools.
// Removed from #[ignore] when the corresponding phase lands.

use ai::domain::smart_preprocessor::{
    clamp_local_tools_to_allowlist, project_safe_local_tools, LocalToolHint, SmartToolHint,
};
use ai::domain::{resolve_tools, ConfigToolsTokens};
use aibe_protocol::{sanitize_readonly_advisory_tools, APPLY_PATCH, WRITE_FILE};

#[test]
fn full_tool_category_excludes_write_tools() {
    let resolved = resolve_tools(Some("@full"), &ConfigToolsTokens::default()).expect("resolve");
    for name in resolved.allowlist.names() {
        assert_ne!(name.as_str(), WRITE_FILE);
        assert_ne!(name.as_str(), APPLY_PATCH);
    }
}

#[test]
fn route_turn_does_not_recommend_write_tools() {
    let safe = sanitize_readonly_advisory_tools(&[
        "read_file".into(),
        WRITE_FILE.into(),
        APPLY_PATCH.into(),
        "grep".into(),
    ]);
    assert!(!safe.iter().any(|t| t == WRITE_FILE || t == APPLY_PATCH));

    for hint in [
        SmartToolHint::GitStatus,
        SmartToolHint::GitDiff,
        SmartToolHint::Grep,
        SmartToolHint::ReadFile,
        SmartToolHint::ListDir,
        SmartToolHint::ShellExecCandidate,
        SmartToolHint::MemorySearch,
        SmartToolHint::ConversationSearch,
    ] {
        if let Some(local) = LocalToolHint::from_smart_tool_hint(hint) {
            let runtime = local.runtime_tool_name();
            assert_ne!(runtime, WRITE_FILE);
            assert_ne!(runtime, APPLY_PATCH);
        }
    }

    let projected = project_safe_local_tools(&[
        SmartToolHint::ReadFile,
        SmartToolHint::Grep,
        SmartToolHint::ShellExecCandidate,
    ]);
    let allowlist = vec![
        "read_file".into(),
        WRITE_FILE.into(),
        APPLY_PATCH.into(),
        "grep".into(),
    ];
    let enabled = clamp_local_tools_to_allowlist(projected, &allowlist);
    assert!(enabled
        .iter()
        .all(|tool| tool.runtime_tool_name() != WRITE_FILE
            && tool.runtime_tool_name() != APPLY_PATCH));
}

#[test]
fn edit_tool_category_includes_write_tools() {
    let resolved = resolve_tools(Some("@edit"), &ConfigToolsTokens::default()).expect("resolve");
    let names: Vec<_> = resolved
        .allowlist
        .names()
        .iter()
        .map(|n| n.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "read_file",
            "list_dir",
            "grep",
            "git_diff",
            "git_status",
            WRITE_FILE,
            APPLY_PATCH,
        ]
    );
    let full = resolve_tools(Some("@full"), &ConfigToolsTokens::default()).expect("resolve");
    for name in full.allowlist.names() {
        assert_ne!(name.as_str(), WRITE_FILE);
        assert_ne!(name.as_str(), APPLY_PATCH);
    }
}

#[test]
fn ai_warns_when_write_tools_enabled() {
    let resolved = resolve_tools(Some("@edit"), &ConfigToolsTokens::default()).expect("resolve");
    assert!(resolved.startup.warn_write);
    assert!(resolved.startup.enabled_list.contains(WRITE_FILE));
    assert!(resolved.startup.enabled_list.contains(APPLY_PATCH));
    assert_eq!(resolved.startup.source_hint.as_deref(), Some("@edit"));

    let literal =
        resolve_tools(Some("write_file"), &ConfigToolsTokens::default()).expect("resolve");
    assert!(literal.startup.warn_write);
    assert_eq!(literal.startup.enabled_list, WRITE_FILE);
}

#[test]
#[ignore = "0054 phase 8: approval_ui_escapes_control_chars"]
fn file_write_approval_ui_escapes_control_chars() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 8: approval_ui_no_continues_turn"]
fn file_write_approval_ui_no_continues_turn() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 8: approval_ui_non_tty"]
fn file_write_approval_ui_rejects_non_tty() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 8: approval_ui_stderr_only"]
fn file_write_approval_ui_writes_stderr_only() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 8: approval_ui_truncation_notice"]
fn file_write_approval_ui_shows_truncation_notice() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 8: approval_ui_yes_executes"]
fn file_write_approval_ui_yes_executes_write() {
    panic!("0054 not implemented");
}

#[test]
#[ignore = "0054 phase 8: verbose_tools_change_id"]
fn verbose_tools_shows_change_id() {
    panic!("0054 not implemented");
}
