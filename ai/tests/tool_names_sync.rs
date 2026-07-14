//! `ai` のツール allowlist が `aibe` 公開名を受け付けること。

use ai::domain::{resolve_tools, ConfigToolsTokens};
use aibe_protocol::HUMAN_TASK;

#[test]
fn every_aibe_known_tool_is_accepted() {
    for name in aibe_protocol::KNOWN_TOOLS {
        // `human_task` は Collaborative Mode 専用。CLI `--tools` 直指定は
        // Normal で fail-closed（UnknownTool）とし、mode policy 経由でのみ公開する。
        if *name == HUMAN_TASK {
            let err = resolve_tools(Some(name), &ConfigToolsTokens::default()).unwrap_err();
            assert!(
                matches!(err, ai::domain::ToolsResolveError::UnknownTool(_)),
                "human_task must stay mode-gated: {err:?}"
            );
            continue;
        }
        let r = resolve_tools(Some(name), &ConfigToolsTokens::default());
        assert!(r.is_ok(), "unknown to ai resolver: {name}");
    }
}
