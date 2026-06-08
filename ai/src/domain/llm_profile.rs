//! LLM プロファイル名の解決（aibe 設定は読まない）。

/// CLI / env / ai 設定の優先順位でプロファイル名を決定する。
///
/// `env_profile` は composition root（`main.rs`）で `std::env::var("AI_LLM_PROFILE")` を読んだ値。
/// いずれも未設定のときは `None`（wire に載せず aibe の `default_profile` に委譲）。
pub fn resolve_llm_profile(
    cli: Option<&str>,
    env_profile: Option<&str>,
    config_default: Option<&str>,
) -> Option<String> {
    if let Some(p) = cli.filter(|s| !s.is_empty()) {
        return Some(p.to_string());
    }
    if let Some(env) = env_profile.filter(|s| !s.is_empty()) {
        return Some(env.to_string());
    }
    config_default.filter(|s| !s.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_config() {
        assert_eq!(
            resolve_llm_profile(Some("cli"), None, Some("cfg")),
            Some("cli".into())
        );
    }

    #[test]
    fn env_overrides_config_when_cli_none() {
        assert_eq!(
            resolve_llm_profile(None, Some("env"), Some("cfg")),
            Some("env".into())
        );
    }

    #[test]
    fn config_used_when_cli_and_env_none() {
        assert_eq!(
            resolve_llm_profile(None, None, Some("cfg")),
            Some("cfg".into())
        );
    }

    #[test]
    fn all_unset_returns_none() {
        assert_eq!(resolve_llm_profile(None, None, None), None);
    }
}
