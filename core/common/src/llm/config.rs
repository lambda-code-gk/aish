//! profiles.json 用の設定型
//!
//! プロバイダ名から ProviderType とオプション（base_url / model / api_key_env / temperature）を解決するための構造体。

use serde::Deserialize;
use std::collections::HashMap;

/// profiles.json のルート
#[derive(Debug, Clone, Default)]
pub struct ProfilesConfig {
    /// 未指定時に使うプロバイダ名
    pub default_provider: Option<String>,
    /// プロバイダ名 -> プロファイル
    pub providers: HashMap<String, ProviderProfile>,
}

/// 1 プロバイダ分の設定
#[derive(Debug, Clone)]
pub struct ProviderProfile {
    /// プロバイダ種別: gemini | openai | openai_compat | echo
    pub type_: ProviderTypeKind,
    /// API のベース URL（省略時は各プロバイダのデフォルト）
    pub base_url: Option<String>,
    /// モデル名（省略時は各プロバイダのデフォルト）
    pub model: Option<String>,
    /// API キーを読む環境変数名（省略時は各プロバイダのデフォルト）
    pub api_key_env: Option<String>,
    /// 温度（0.0〜1.0 等、省略時はデフォルト）
    pub temperature: Option<f32>,
}

/// JSON の "type" で使うプロバイダ種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTypeKind {
    Gemini,
    Openai,
    OpenaiCompat,
    Echo,
}

impl ProviderTypeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Openai => "openai",
            Self::OpenaiCompat => "openai_compat",
            Self::Echo => "echo",
        }
    }
}

/// serde 用の内部構造（type が予約語のため）
#[derive(Debug, Deserialize)]
struct ProfilesConfigRaw {
    #[serde(alias = "default")]
    default_provider: Option<String>,
    providers: Option<HashMap<String, ProviderProfileRaw>>,
}

#[derive(Debug, Deserialize)]
struct ProviderProfileRaw {
    #[serde(rename = "type", alias = "provider")]
    type_: ProviderTypeKindSerde,
    base_url: Option<String>,
    #[serde(alias = "default_model")]
    model: Option<String>,
    api_key_env: Option<String>,
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ProviderTypeKindSerde {
    Gemini,
    #[serde(alias = "gpt")]
    Openai,
    #[serde(rename = "openai_compat", alias = "ollama")]
    OpenaiCompat,
    Echo,
}

impl From<ProviderTypeKindSerde> for ProviderTypeKind {
    fn from(s: ProviderTypeKindSerde) -> Self {
        match s {
            ProviderTypeKindSerde::Gemini => ProviderTypeKind::Gemini,
            ProviderTypeKindSerde::Openai => ProviderTypeKind::Openai,
            ProviderTypeKindSerde::OpenaiCompat => ProviderTypeKind::OpenaiCompat,
            ProviderTypeKindSerde::Echo => ProviderTypeKind::Echo,
        }
    }
}

impl ProfilesConfig {
    /// JSON 文字列からパース（ファイル読みは resolver で行う）
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        let raw: ProfilesConfigRaw = serde_json::from_str(json)?;
        let providers = raw
            .providers
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();
        Ok(ProfilesConfig {
            default_provider: raw.default_provider,
            providers,
        })
    }
}

impl From<ProviderProfileRaw> for ProviderProfile {
    fn from(r: ProviderProfileRaw) -> Self {
        ProviderProfile {
            type_: r.type_.into(),
            base_url: r.base_url,
            model: r.model,
            api_key_env: r.api_key_env,
            temperature: r.temperature,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_object() {
        let cfg = ProfilesConfig::parse("{}").unwrap();
        assert!(cfg.default_provider.is_none());
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn test_parse_default_provider_and_providers() {
        let json = r#"
        {
            "default_provider": "my_gemini",
            "providers": {
                "my_gemini": { "type": "gemini", "model": "gemini-2.0" },
                "my_openai": { "type": "openai", "api_key_env": "OPENAI_KEY" },
                "local": { "type": "openai_compat", "base_url": "http://localhost:8080/v1" },
                "echo": { "type": "echo" }
            }
        }
        "#;
        let cfg = ProfilesConfig::parse(json).unwrap();
        assert_eq!(cfg.default_provider.as_deref(), Some("my_gemini"));
        assert_eq!(cfg.providers.len(), 4);

        let g = cfg.providers.get("my_gemini").unwrap();
        assert!(matches!(g.type_, ProviderTypeKind::Gemini));
        assert_eq!(g.model.as_deref(), Some("gemini-2.0"));

        let o = cfg.providers.get("my_openai").unwrap();
        assert!(matches!(o.type_, ProviderTypeKind::Openai));
        assert_eq!(o.api_key_env.as_deref(), Some("OPENAI_KEY"));

        let l = cfg.providers.get("local").unwrap();
        assert!(matches!(l.type_, ProviderTypeKind::OpenaiCompat));
        assert_eq!(l.base_url.as_deref(), Some("http://localhost:8080/v1"));

        let e = cfg.providers.get("echo").unwrap();
        assert!(matches!(e.type_, ProviderTypeKind::Echo));
    }

    #[test]
    fn test_parse_type_alias_gpt() {
        let json = r#"{ "providers": { "x": { "type": "gpt" } } }"#;
        let cfg = ProfilesConfig::parse(json).unwrap();
        let p = cfg.providers.get("x").unwrap();
        assert!(matches!(p.type_, ProviderTypeKind::Openai));
    }

    #[test]
    fn test_parse_alias_default_and_default_model_and_ollama() {
        // サンプル profiles.json 互換: default_provider→default, model→default_model, type→ollama
        let json = r#"
        {
            "default": "local",
            "providers": {
                "local": {
                    "type": "ollama",
                    "base_url": "http://localhost:11434/v1",
                    "default_model": "llama3.1",
                    "temperature": 0.4
                }
            }
        }
        "#;
        let cfg = ProfilesConfig::parse(json).unwrap();
        assert_eq!(cfg.default_provider.as_deref(), Some("local"));
        let p = cfg.providers.get("local").unwrap();
        assert!(matches!(p.type_, ProviderTypeKind::OpenaiCompat));
        assert_eq!(p.model.as_deref(), Some("llama3.1"));
        assert_eq!(p.base_url.as_deref(), Some("http://localhost:11434/v1"));
        assert_eq!(p.temperature, Some(0.4));
    }
}
