//! `ai` カテゴリ表と `aibe::KNOWN_TOOLS` の同期（`docs/done/0009_ai-tool-category-sync-spec.md`）。

use std::collections::BTreeSet;

use ai::domain::{resolve_tools, ConfigToolsTokens};
use aibe::{KNOWN_TOOLS, READ_FILE, SHELL_EXEC};

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
    assert_category_eq("@read-only", &[READ_FILE]);
}

#[test]
fn exec_category_expands() {
    assert_category_eq("@exec", &[SHELL_EXEC]);
}

#[test]
fn full_category_expands_in_fixed_order() {
    assert_category_eq("@full", &[READ_FILE, SHELL_EXEC]);
}

#[test]
fn full_category_covers_all_known_tools() {
    let expanded = resolve_category("@full");
    let expanded_set: BTreeSet<_> = expanded.iter().map(String::as_str).collect();
    let known_set: BTreeSet<_> = KNOWN_TOOLS.iter().copied().collect();

    let missing: Vec<_> = known_set.difference(&expanded_set).copied().collect();
    let extra: Vec<_> = expanded_set.difference(&known_set).copied().collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "@full must expand to exactly aibe::KNOWN_TOOLS.\n\
         missing from @full: {missing:?}\n\
         extra in @full (not in KNOWN_TOOLS): {extra:?}\n\
         update ai/src/domain/tools.rs expand_category, docs/done/0002 カテゴリ表, \
         and assign the new tool to a category; see {CHECKLIST}"
    );
}

#[test]
fn every_expanded_name_is_known_to_aibe() {
    for category in ["@read-only", "@exec", "@full"] {
        for name in resolve_category(category) {
            assert!(
                aibe::is_known_tool(&name),
                "category {category} expanded to unknown tool {name}; \
                 see {CHECKLIST}"
            );
        }
    }
}
