use std::collections::BTreeSet;

use ai::domain::{resolve_tools, ConfigToolsTokens};
use aibe_protocol::{
    is_known_tool, ToolName, GIT_DIFF, GIT_STATUS, GREP, LIST_DIR, READ_FILE, SHELL_EXEC,
};

const CHECKLIST: &str = "docs/manual/ai-ask-tools.md#新規組み込みツール追加チェックリスト";

fn resolve_category(category: &str) -> Vec<String> {
    resolve_tools(Some(category), &ConfigToolsTokens::default())
        .expect("category should resolve")
        .allowlist
        .names()
        .iter()
        .map(|n| n.as_str().to_string())
        .collect()
}

fn assert_category_eq(category: &str, expected: &[&str]) {
    let resolved = resolve_category(category);
    let got: Vec<&str> = resolved.iter().map(String::as_str).collect();
    assert_eq!(
        got.as_slice(),
        expected,
        "category {category} expansion mismatch (expected {expected:?}, got {got:?}); \
         update ai/src/domain/tools.rs expand_category and docs/done/0002 カテゴリ表; \
         see {CHECKLIST}"
    );
}

#[test]
fn read_only_category_expands() {
    assert_category_eq(
        "@read-only",
        &[READ_FILE, LIST_DIR, GREP, GIT_DIFF, GIT_STATUS],
    );
}

#[test]
fn exec_category_expands() {
    assert_category_eq("@exec", &[SHELL_EXEC]);
}

#[test]
fn full_category_expands_in_fixed_order() {
    assert_category_eq("@full", &[READ_FILE, LIST_DIR, GREP, GIT_DIFF, GIT_STATUS]);
}

#[test]
fn full_category_does_not_include_shell_exec() {
    let expanded = resolve_category("@full");
    assert!(
        !expanded.iter().any(|name| name == SHELL_EXEC),
        "@full must not include shell_exec; see {CHECKLIST}"
    );
}

#[test]
fn safe_tools_are_accepted_as_literals() {
    let resolved = resolve_tools(
        Some("read_file,list_dir,grep,git_diff,git_status"),
        &ConfigToolsTokens::default(),
    )
    .expect("resolve");
    assert_eq!(
        resolved.allowlist.names(),
        &[
            ToolName::read_file(),
            ToolName::list_dir(),
            ToolName::grep(),
            ToolName::git_diff(),
            ToolName::git_status(),
        ]
    );
}

#[test]
fn shell_exec_literal_is_still_known() {
    let resolved =
        resolve_tools(Some("shell_exec"), &ConfigToolsTokens::default()).expect("resolve");
    assert_eq!(resolved.allowlist.names(), &[ToolName::shell_exec()]);
    assert!(resolved.startup.warn_shell);
}

#[test]
fn every_expanded_name_is_known_to_aibe() {
    for category in ["@read-only", "@exec", "@full"] {
        for name in resolve_category(category) {
            assert!(
                is_known_tool(&name),
                "category {category} expanded to unknown tool {name}; \
                 see {CHECKLIST}"
            );
        }
    }
}

#[test]
fn categories_cover_safe_tools_without_shell() {
    let full: BTreeSet<_> = resolve_category("@full")
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let safe: BTreeSet<_> = [READ_FILE, LIST_DIR, GREP, GIT_DIFF, GIT_STATUS]
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(full, safe);
}
