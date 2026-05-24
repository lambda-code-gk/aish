//! `ai` のツール allowlist が `aibe` 公開名を受け付けること。

use ai::domain::{resolve_tools, ConfigToolsTokens};

#[test]
fn every_aibe_known_tool_is_accepted() {
    for name in aibe::KNOWN_TOOLS {
        let r = resolve_tools(Some(name), &ConfigToolsTokens::default());
        assert!(r.is_ok(), "unknown to ai resolver: {name}");
    }
}
