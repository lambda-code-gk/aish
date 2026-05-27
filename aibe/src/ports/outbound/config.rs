//! サーバ設定 outbound port。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

use super::llm::LlmProvider;
use super::termination_capability::TerminationCapability;

/// `tool_calls` / LLM 向け tool result の既定上限（バイト）。
pub const DEFAULT_MAX_TOOL_OUTPUT_BYTES: usize = aibe_protocol::MAX_TOOL_OUTPUT_BYTES;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub socket_path: PathBuf,
    pub llm: LlmProfilesConfig,
    pub tools: ToolsConfig,
}

/// max-round 到達時の終端戦略（policy）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminationStrategy {
    /// 実行記録を要約 user メッセージに圧縮（既定・0003 互換）。
    #[default]
    SummaryPrompt,
    /// capability が許すときループ会話を無加工で `complete()` に渡す。
    ConversationReplay,
}

impl TerminationStrategy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "summary_prompt" => Some(Self::SummaryPrompt),
            "conversation_replay" => Some(Self::ConversationReplay),
            _ => None,
        }
    }
}

/// `[tools] max_rounds` の最小値（TOML 読み込み時に検証）。
pub const MIN_MAX_TOOL_ROUNDS: u32 = 1;

/// ツール実行とエージェントループの設定。
#[derive(Debug, Clone)]
pub struct ToolsConfig {
    pub max_rounds: u32,
    pub exec_timeout_ms: u64,
    /// `tool_calls` / LLM 向け tool result の最大バイト数。
    pub max_tool_output_bytes: usize,
    pub termination_strategy: TerminationStrategy,
    pub shell_exec: ShellExecConfig,
    pub read_file: ReadFileConfig,
}

#[derive(Debug, Clone)]
pub struct ShellExecConfig {
    pub enabled: bool,
    pub allowed_commands: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReadFileConfig {
    pub allowed_roots: Vec<PathBuf>,
}

impl ToolsConfig {
    /// 1 `agent_turn` あたりの LLM↔tool ループ上限。
    ///
    /// - `config.toml` で `max_rounds = 0` は [`ConfigError::Invalid`]（読み込み拒否）。
    /// - プログラムから `max_rounds: 0` が渡された場合のみ **1 に補正**（無限ループ防止の安全網）。
    pub fn effective_max_rounds(&self) -> u32 {
        self.max_rounds.max(MIN_MAX_TOOL_ROUNDS)
    }
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            max_rounds: 8,
            exec_timeout_ms: 30_000,
            max_tool_output_bytes: DEFAULT_MAX_TOOL_OUTPUT_BYTES,
            termination_strategy: TerminationStrategy::default(),
            shell_exec: ShellExecConfig {
                enabled: true,
                allowed_commands: vec![],
            },
            read_file: ReadFileConfig {
                allowed_roots: vec![PathBuf::from(".")],
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProviderKind {
    Mock,
    OpenAiCompatible,
    Gemini,
}

#[derive(Debug, Clone)]
pub struct LlmBackend {
    pub provider: LlmProviderKind,
    pub api_key: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct LlmGenerationParams {
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LlmProfile {
    pub llm: String,
    pub model: String,
    pub params: LlmGenerationParams,
}

#[derive(Debug, Clone)]
pub struct LlmProfilesConfig {
    pub backends: HashMap<String, LlmBackend>,
    pub profiles: HashMap<String, LlmProfile>,
    pub default_profile: String,
}

impl LlmProfilesConfig {
    /// 設定ファイル無し・LLM 節無し時の既定（mock 1 プロファイル）。
    pub fn default_mock() -> Self {
        let mut backends = HashMap::new();
        backends.insert(
            "default".to_string(),
            LlmBackend {
                provider: LlmProviderKind::Mock,
                api_key: String::new(),
                base_url: String::new(),
            },
        );
        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            LlmProfile {
                llm: "default".to_string(),
                model: "mock".to_string(),
                params: LlmGenerationParams::default(),
            },
        );
        Self {
            backends,
            profiles,
            default_profile: "default".to_string(),
        }
    }
}

/// 起動時 eager 構築。リクエスト時は参照のみ。
pub struct ProfileRegistry {
    pub providers: HashMap<String, Arc<dyn LlmProvider>>,
    pub capabilities: HashMap<String, TerminationCapability>,
    pub default_profile: String,
}

impl ProfileRegistry {
    pub fn resolve(
        &self,
        llm_profile: Option<&str>,
    ) -> Result<(&Arc<dyn LlmProvider>, TerminationCapability), String> {
        let name = llm_profile.unwrap_or(&self.default_profile);
        let provider = self
            .providers
            .get(name)
            .ok_or_else(|| format!("unknown llm profile: {name}"))?;
        let capability = self
            .capabilities
            .get(name)
            .copied()
            .unwrap_or_else(TerminationCapability::summary_prompt_only);
        Ok((provider, capability))
    }

    /// テスト用: 単一プロファイルのレジストリ。
    pub fn single(
        profile_name: impl Into<String>,
        llm: Arc<dyn LlmProvider>,
        capability: TerminationCapability,
    ) -> Self {
        let profile_name = profile_name.into();
        let mut providers = HashMap::new();
        providers.insert(profile_name.clone(), llm);
        let mut capabilities = HashMap::new();
        capabilities.insert(profile_name.clone(), capability);
        Self {
            providers,
            capabilities,
            default_profile: profile_name,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid configuration: {0}")]
    Invalid(String),
    #[error("failed to read config: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_max_rounds_clamps_zero_to_one() {
        let cfg = ToolsConfig {
            max_rounds: 0,
            ..ToolsConfig::default()
        };
        assert_eq!(cfg.effective_max_rounds(), 1);
    }

    #[test]
    fn effective_max_rounds_preserves_positive() {
        let cfg = ToolsConfig {
            max_rounds: 3,
            ..ToolsConfig::default()
        };
        assert_eq!(cfg.effective_max_rounds(), 3);
    }
}

/// 設定の読み込み。
pub trait ConfigLoader {
    fn load(&self) -> Result<AppConfig, ConfigError>;
}
