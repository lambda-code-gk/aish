//! LLM プロファイル名の解決（aibe 設定は読まない）。

/// CLI / env / ai 設定の優先順位でプロファイル名を決定する。
///
/// いずれも未設定のときは `None`（wire に載せず aibe の `default_profile` に委譲）。
pub fn resolve_llm_profile(cli: Option<&str>, config_default: Option<&str>) -> Option<String> {
    if let Some(p) = cli.filter(|s| !s.is_empty()) {
        return Some(p.to_string());
    }
    if let Ok(env) = std::env::var("AI_LLM_PROFILE") {
        if !env.is_empty() {
            return Some(env);
        }
    }
    config_default.filter(|s| !s.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_config() {
        assert_eq!(
            resolve_llm_profile(Some("cli"), Some("cfg")),
            Some("cli".into())
        );
    }

    #[test]
    fn config_used_when_cli_none() {
        assert_eq!(resolve_llm_profile(None, Some("cfg")), Some("cfg".into()));
    }

    #[test]
    fn all_unset_returns_none() {
        assert_eq!(resolve_llm_profile(None, None), None);
    }
}
