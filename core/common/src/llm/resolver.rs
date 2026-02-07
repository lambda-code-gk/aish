//! profiles.json の読み込みとプロバイダ解決

use crate::domain::ProviderName;
use crate::error::Error;
use crate::llm::config::{ProfilesConfig, ProviderTypeKind};
use crate::llm::factory::ProviderType;
use crate::ports::outbound::{EnvResolver, FileSystem};

/// 解決済みプロバイダ（ProviderType + オプション）
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    /// 解決に使ったプロファイル名（例: "local", "gemini"）。エラー表示用
    pub profile_name: String,
    pub provider_type: ProviderType,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
    pub temperature: Option<f32>,
}

/// profiles.json を読み込む。ファイルが無ければ Ok(None)、JSON が壊れていれば Err（メッセージにパス含める）
pub fn load_profiles_config(
    fs: &dyn FileSystem,
    env: &dyn EnvResolver,
) -> Result<Option<ProfilesConfig>, Error> {
    let path = env.resolve_profiles_config_path()?;
    if !fs.exists(path.as_path()) {
        return Ok(None);
    }
    let contents = fs
        .read_to_string(path.as_path())
        .map_err(|e| Error::io_msg(format!("{}: {}", path.display(), e)))?;
    ProfilesConfig::parse(&contents)
        .map_err(|e| Error::json(format!("{}: {}", path.display(), e)))
        .map(Some)
}

fn provider_type_kind_to_provider_type(k: ProviderTypeKind) -> ProviderType {
    match k {
        ProviderTypeKind::Gemini => ProviderType::Gemini,
        ProviderTypeKind::Openai => ProviderType::Gpt,
        ProviderTypeKind::OpenaiCompat => ProviderType::OpenAiCompat,
        ProviderTypeKind::Echo => ProviderType::Echo,
    }
}

/// 利用可能なビルトインプロバイダ名
fn builtin_provider_names() -> &'static [&'static str] {
    &["gemini", "gpt", "openai", "openai_compat", "echo"]
}

/// 現在有効な（利用可能な）プロファイル一覧。名前のソート済みリストとデフォルトプロファイル名。
/// cfg が Some なら profiles.json のキーのみ、None ならビルトイン名のみを返す。
pub fn list_available_profiles(cfg: Option<&ProfilesConfig>) -> (Vec<String>, Option<String>) {
    let mut names: Vec<String> = if let Some(c) = cfg {
        c.providers.keys().cloned().collect()
    } else {
        builtin_provider_names()
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    };
    names.sort();
    let default = cfg.and_then(|c| c.default_provider.clone());
    (names, default)
}

/// 要求されたプロバイダ名（None の場合は default）と ProfilesConfig から ResolvedProvider を解決する。
/// 不明なプロバイダの場合は Error::invalid_argument（is_usage == true）で利用可能一覧を返す。
pub fn resolve_provider(
    requested: Option<&ProviderName>,
    cfg: Option<&ProfilesConfig>,
) -> Result<ResolvedProvider, Error> {
    let effective_name: &str = requested
        .map(|r| r.as_ref())
        .unwrap_or_else(|| {
            cfg.and_then(|c| c.default_provider.as_deref())
                .unwrap_or("gemini")
        });

    // 1) cfg.providers に名前があればそれを優先
    if let Some(cfg) = cfg {
        if let Some(profile) = cfg.providers.get(effective_name) {
            let provider_type = provider_type_kind_to_provider_type(profile.type_);
            return Ok(ResolvedProvider {
                profile_name: effective_name.to_string(),
                provider_type,
                base_url: profile.base_url.clone(),
                model: profile.model.clone(),
                api_key_env: profile.api_key_env.clone(),
                temperature: profile.temperature,
            });
        }
    }

    // 2) profiles.json が無い場合のみ、ビルトインにフォールバック（後方互換）
    if cfg.is_none() {
        if let Some(provider_type) = ProviderType::from_str(effective_name) {
            return Ok(ResolvedProvider {
                profile_name: effective_name.to_string(),
                provider_type,
                base_url: None,
                model: None,
                api_key_env: None,
                temperature: None,
            });
        }
    }

    // 3) どれも無ければ usage エラー
    let (available, _) = list_available_profiles(cfg);
    Err(Error::invalid_argument(format!(
        "Unknown provider: '{}'. Available: {}",
        effective_name,
        available.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ProviderName;
    use crate::llm::config::{ProfilesConfig, ProviderProfile, ProviderTypeKind};

    #[test]
    fn test_resolve_provider_no_cfg_requested_none() {
        let r = resolve_provider(None, None).unwrap();
        assert_eq!(r.profile_name, "gemini");
        assert_eq!(r.provider_type, ProviderType::Gemini);
        assert!(r.model.is_none());
    }

    #[test]
    fn test_resolve_provider_no_cfg_requested_gemini() {
        let name = ProviderName::new("gemini");
        let r = resolve_provider(Some(&name), None).unwrap();
        assert_eq!(r.provider_type, ProviderType::Gemini);
    }

    #[test]
    fn test_resolve_provider_no_cfg_requested_gpt() {
        let name = ProviderName::new("gpt");
        let r = resolve_provider(Some(&name), None).unwrap();
        assert_eq!(r.provider_type, ProviderType::Gpt);
    }

    #[test]
    fn test_resolve_provider_no_cfg_requested_echo() {
        let name = ProviderName::new("echo");
        let r = resolve_provider(Some(&name), None).unwrap();
        assert_eq!(r.provider_type, ProviderType::Echo);
    }

    #[test]
    fn test_resolve_provider_no_cfg_unknown() {
        let name = ProviderName::new("unknown_provider");
        let e = resolve_provider(Some(&name), None).unwrap_err();
        assert!(e.is_usage());
        assert!(e.to_string().contains("Unknown provider"));
        assert!(e.to_string().contains("unknown_provider"));
        assert!(e.to_string().contains("Available"));
    }

    #[test]
    fn test_resolve_provider_cfg_default_provider() {
        let cfg = ProfilesConfig {
            default_provider: Some("my_openai".to_string()),
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "my_openai".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Openai,
                        base_url: Some("https://my.api/v1".to_string()),
                        model: Some("gpt-4".to_string()),
                        api_key_env: Some("MY_KEY".to_string()),
                        temperature: Some(0.7),
                    },
                );
                m
            },
        };
        let r = resolve_provider(None, Some(&cfg)).unwrap();
        assert_eq!(r.profile_name, "my_openai");
        assert_eq!(r.provider_type, ProviderType::Gpt);
        assert_eq!(r.base_url.as_deref(), Some("https://my.api/v1"));
        assert_eq!(r.model.as_deref(), Some("gpt-4"));
        assert_eq!(r.api_key_env.as_deref(), Some("MY_KEY"));
        assert_eq!(r.temperature, Some(0.7));
    }

    #[test]
    fn test_resolve_provider_cfg_requested_overrides_default() {
        let cfg = ProfilesConfig {
            default_provider: Some("gemini".to_string()),
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "gemini".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Gemini,
                        base_url: None,
                        model: None,
                        api_key_env: None,
                        temperature: None,
                    },
                );
                m.insert(
                    "echo".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Echo,
                        base_url: None,
                        model: None,
                        api_key_env: None,
                        temperature: None,
                    },
                );
                m
            },
        };
        let name = ProviderName::new("echo");
        let r = resolve_provider(Some(&name), Some(&cfg)).unwrap();
        assert_eq!(r.provider_type, ProviderType::Echo);
    }

    #[test]
    fn test_resolve_provider_cfg_custom_name_unknown_builtin_fallback() {
        let cfg = ProfilesConfig {
            default_provider: None,
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "custom_gemini".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Gemini,
                        base_url: None,
                        model: Some("gemini-2.0".to_string()),
                        api_key_env: None,
                        temperature: None,
                    },
                );
                m
            },
        };
        let name = ProviderName::new("custom_gemini");
        let r = resolve_provider(Some(&name), Some(&cfg)).unwrap();
        assert_eq!(r.provider_type, ProviderType::Gemini);
        assert_eq!(r.model.as_deref(), Some("gemini-2.0"));
    }

    #[test]
    fn test_resolve_provider_cfg_unknown_provider_lists_available() {
        let cfg = ProfilesConfig {
            default_provider: None,
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "my_custom".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Echo,
                        base_url: None,
                        model: None,
                        api_key_env: None,
                        temperature: None,
                    },
                );
                m
            },
        };
        let name = ProviderName::new("nonexistent");
        let e = resolve_provider(Some(&name), Some(&cfg)).unwrap_err();
        assert!(e.is_usage());
        let msg = e.to_string();
        assert!(msg.contains("Unknown provider"));
        assert!(msg.contains("nonexistent"));
        assert!(msg.contains("my_custom"));
        // cfg がある場合、ビルトイン名は一覧に含まれない
        assert!(!msg.contains("gemini"));
    }

    #[test]
    fn test_resolve_provider_cfg_present_no_builtin_fallback() {
        // profiles.json がある場合、ビルトイン名でも profiles.json に無ければエラー
        let cfg = ProfilesConfig {
            default_provider: None,
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "my_echo".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Echo,
                        base_url: None,
                        model: None,
                        api_key_env: None,
                        temperature: None,
                    },
                );
                m
            },
        };
        let name = ProviderName::new("echo");
        let e = resolve_provider(Some(&name), Some(&cfg)).unwrap_err();
        assert!(e.is_usage());
        assert!(e.to_string().contains("Unknown provider"));
        assert!(e.to_string().contains("echo"));
    }

    #[test]
    fn test_list_available_profiles_no_cfg() {
        let (names, default) = list_available_profiles(None);
        assert_eq!(default, None);
        assert!(names.contains(&"gemini".to_string()));
        assert!(names.contains(&"echo".to_string()));
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_list_available_profiles_with_cfg() {
        let cfg = ProfilesConfig {
            default_provider: Some("my_openai".to_string()),
            providers: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "my_openai".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Openai,
                        base_url: None,
                        model: None,
                        api_key_env: None,
                        temperature: None,
                    },
                );
                m.insert(
                    "custom_echo".to_string(),
                    ProviderProfile {
                        type_: ProviderTypeKind::Echo,
                        base_url: None,
                        model: None,
                        api_key_env: None,
                        temperature: None,
                    },
                );
                m
            },
        };
        let (names, default) = list_available_profiles(Some(&cfg));
        assert_eq!(default.as_deref(), Some("my_openai"));
        assert!(names.contains(&"my_openai".to_string()));
        assert!(names.contains(&"custom_echo".to_string()));
        // cfg がある場合、ビルトイン名は含まれない
        assert!(!names.contains(&"gemini".to_string()));
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
