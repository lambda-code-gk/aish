//! `ai ask` 起動計画（tools 解決を socket 接続より前に固定する）。

use std::path::{Path, PathBuf};

use crate::domain::{resolve_tools, ConfigToolsTokens, ResolvedTools, ToolsResolveError};

/// tools 解決後に aibe へ接続するときのパラメータ。
#[derive(Debug, Clone)]
pub struct AskLaunchPlan {
    pub socket_path: PathBuf,
    pub resolved_tools: ResolvedTools,
    pub auto_start: bool,
}

/// config / CLI から allowlist を解決する。失敗時は `ensure_running` を呼ばないこと。
pub fn plan_ask_launch(
    ask_tools: &ConfigToolsTokens,
    tools_cli: Option<&str>,
    socket_path: PathBuf,
    auto_start: bool,
) -> Result<AskLaunchPlan, ToolsResolveError> {
    let resolved_tools = resolve_tools(tools_cli, ask_tools)?;
    Ok(AskLaunchPlan {
        socket_path,
        resolved_tools,
        auto_start,
    })
}

/// `auto_start` のときだけ `ensure_running` を実行する。
pub fn ensure_aibe_if_needed<E>(
    plan: &AskLaunchPlan,
    ensure_running: impl FnOnce(&Path) -> Result<(), E>,
) -> Result<(), E> {
    if plan.auto_start {
        ensure_running(&plan.socket_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_error_skips_ensure_running() {
        let cfg = ConfigToolsTokens(vec!["nope".into()]);
        assert!(plan_ask_launch(&cfg, None, PathBuf::from("/tmp/t.sock"), true).is_err());

        let mut ensure_called = false;
        assert!(plan_ask_launch(&cfg, Some("nope"), PathBuf::from("/tmp/t.sock"), true).is_err());
        assert!(!ensure_called);

        let plan = plan_ask_launch(&cfg, Some("@read-only"), PathBuf::from("/tmp/t.sock"), true)
            .expect("plan");
        ensure_aibe_if_needed(&plan, |_| {
            ensure_called = true;
            Ok::<(), ()>(())
        })
        .expect("ensure");
        assert!(ensure_called);
    }

    #[test]
    fn cli_none_overrides_config_in_plan() {
        let cfg = ConfigToolsTokens(vec!["@read-only".into()]);
        let plan =
            plan_ask_launch(&cfg, Some("none"), PathBuf::from("/tmp/t.sock"), false).expect("plan");
        assert!(plan.resolved_tools.names.is_empty());
    }
}
