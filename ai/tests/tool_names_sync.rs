//! `ai` ドメインのツール名が `aibe` 公開名と一致すること。

use ai::domain::{resolve_tools, ConfigToolsTokens, READ_FILE, SHELL_EXEC};

#[test]
fn tool_names_match_aibe() {
    assert_eq!(READ_FILE, aibe::READ_FILE);
    assert_eq!(SHELL_EXEC, aibe::SHELL_EXEC);
}

#[test]
fn every_aibe_known_tool_is_accepted() {
    for name in aibe::KNOWN_TOOLS {
        let r = resolve_tools(Some(name), &ConfigToolsTokens::default());
        assert!(r.is_ok(), "unknown to ai resolver: {name}");
    }
}
