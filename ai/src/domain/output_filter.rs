//! assistant 本文向け output filter の解決。

/// `非空 AI_FILTER` > `非空 [ask].filter` > なし。
pub fn resolve_output_filter(env: Option<String>, config: Option<&str>) -> Option<String> {
    if let Some(env) = env {
        if !env.is_empty() {
            return Some(env);
        }
    }
    config.filter(|s| !s.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_config() {
        assert_eq!(
            resolve_output_filter(Some("env".into()), Some("cfg")),
            Some("env".into())
        );
    }

    #[test]
    fn empty_env_falls_back_to_config() {
        assert_eq!(
            resolve_output_filter(Some(String::new()), Some("cfg")),
            Some("cfg".into())
        );
    }

    #[test]
    fn all_unset_returns_none() {
        assert_eq!(resolve_output_filter(None, None), None);
    }
}
