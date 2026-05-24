//! サーバ設定 outbound port。

use std::path::PathBuf;
use thiserror::Error;

/// `tool_calls` / LLM 向け tool result の既定上限（バイト）。
pub const DEFAULT_MAX_TOOL_OUTPUT_BYTES: usize = 32_768;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub socket_path: PathBuf,
    pub llm: LlmConfig,
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

#[derive(Debug, Clone)]
pub enum LlmConfig {
    Mock,
    OpenAiCompatible {
        base_url: String,
        api_key: String,
        model: String,
    },
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
