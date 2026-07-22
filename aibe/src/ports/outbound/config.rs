//! サーバ設定 outbound port。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

use crate::domain::FeaturePackResolution;

use super::llm::LlmProvider;
use super::termination_capability::TerminationCapability;

/// `tool_calls` / LLM 向け tool result の既定上限（バイト）。
pub const DEFAULT_MAX_TOOL_OUTPUT_BYTES: usize = aibe_protocol::MAX_TOOL_OUTPUT_BYTES;

/// `list_dir` が返すエントリ行の既定上限。
pub const DEFAULT_MAX_LIST_ENTRIES: usize = 10_000;
/// `grep` が走査するファイル数の既定上限。
pub const DEFAULT_MAX_GREP_FILES_SCANNED: usize = 5_000;
/// `grep` が返すマッチ行の既定上限。
pub const DEFAULT_MAX_GREP_MATCHES: usize = 5_000;
/// `grep` が 1 ファイルあたり読むバイトの既定上限。
pub const DEFAULT_MAX_GREP_FILE_BYTES: usize = 1_048_576;

/// `write_file` / `apply_patch` の既定ファイルサイズ上限（バイト）。
pub const DEFAULT_MAX_FILE_WRITE_BYTES: usize = 1_048_576;
/// `apply_patch` の既定 patch サイズ上限（バイト）。
pub const DEFAULT_MAX_PATCH_BYTES: usize = 1_048_576;
/// 承認 UI 向け diff preview の既定上限（バイト）。
pub const DEFAULT_MAX_PREVIEW_BYTES: usize = 32_768;
/// diff preview 生成時の行数上限（超過時は preview を省略）。
pub const DEFAULT_MAX_DIFF_LINES: usize = 10_000;
/// diff preview 生成時の作業量上限（old_lines × new_lines の概算）。
pub const DEFAULT_MAX_DIFF_WORK: usize = 1_000_000;
/// rollback journal の既定保持日数。
pub const DEFAULT_JOURNAL_RETENTION_DAYS: u32 = 7;
/// rollback journal の既定容量上限（バイト）。
pub const DEFAULT_JOURNAL_MAX_BYTES: u64 = 268_435_456;

/// `[[external_commands]]` — `shell_exec` 用の起動テンプレート（first-class tool ではない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCommandConfig {
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

/// `[agent_task]` と `[[agent_task.workers]]` の本番 Worker 設定。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentTaskConfig {
    pub enabled: bool,
    pub workers: Vec<AgentTaskWorkerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskWorkerConfig {
    pub id: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub timeout_secs: u64,
    pub permission_profile: String,
    /// 値ではなく、親環境から継承してよい変数名だけを保持する。
    pub env_allowlist: Vec<String>,
}

/// 外部コマンドの `timeout_secs` 既定（30 分）。
pub const DEFAULT_EXTERNAL_COMMAND_TIMEOUT_SECS: u64 = 1800;

/// contextual memory ランタイムの有効/無効（Phase A: basic profile 切替）。
///
/// `kind_files` / `recipe_files` が `None` のときは互換モード（baseline pack + 従来 override）。
/// `Some(vec![])` を明示したときだけ pack を無効化する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub kind_files: Option<Vec<PathBuf>>,
    pub recipe_files: Option<Vec<PathBuf>>,
    /// smart feature 定義 TOML（0042）。`None` は baseline pack 互換。
    pub feature_files: Option<Vec<PathBuf>>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            kind_files: None,
            recipe_files: None,
            feature_files: None,
        }
    }
}

impl MemoryConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            kind_files: None,
            recipe_files: None,
            feature_files: None,
        }
    }

    /// `kind_files=None` 互換を含め、AISH kind pack が有効か。
    pub fn memory_kinds_enabled(&self) -> bool {
        match &self.kind_files {
            None => true,
            Some(files) => !files.is_empty(),
        }
    }

    /// `recipe_files=None` 互換を含め、recipe pack が有効か。
    pub fn recipes_enabled(&self) -> bool {
        match &self.recipe_files {
            None => true,
            Some(files) => !files.is_empty(),
        }
    }

    /// `kind_files=[]` かつ `recipe_files=[]` の generic memory 明示設定。
    pub fn is_explicit_generic_memory_pack(&self) -> bool {
        matches!(&self.kind_files, Some(k) if k.is_empty())
            && matches!(&self.recipe_files, Some(r) if r.is_empty())
    }

    /// `MemoryConfig` から feature pack 入力を解決する（0043 Phase 3）。
    ///
    /// `memory.enabled=false` のときは呼び出し元が `FeatureRegistry::empty()` を使う想定。
    /// 本メソッドは `enabled` を見ず、`feature_files` と generic memory 状態だけを解釈する。
    pub fn resolve_feature_pack(&self) -> FeaturePackResolution {
        match &self.feature_files {
            Some(files) if files.is_empty() => FeaturePackResolution::empty(),
            Some(files) => FeaturePackResolution::explicit_files(files.clone()),
            None if self.is_explicit_generic_memory_pack() => FeaturePackResolution::empty(),
            None => FeaturePackResolution::baseline_compat(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub socket_path: PathBuf,
    pub conversation_store_root: PathBuf,
    pub router: RouterConfig,
    pub llm: LlmProfilesConfig,
    pub tools: ToolsConfig,
    pub external_commands: Vec<ExternalCommandConfig>,
    pub agent_task: AgentTaskConfig,
    pub memory: MemoryConfig,
}

pub fn validate_agent_task_config(config: &AgentTaskConfig) -> Result<(), ConfigError> {
    let mut ids = std::collections::HashSet::new();
    for worker in &config.workers {
        crate::domain::WorkerId::parse(worker.id.clone())
            .map_err(|e| ConfigError::Invalid(format!("agent_task worker id: {e}")))?;
        if !ids.insert(worker.id.clone()) {
            return Err(ConfigError::Invalid(format!(
                "duplicate agent_task worker id: {}",
                worker.id
            )));
        }
        if worker.executable.as_os_str().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "agent_task worker '{}' executable must not be empty",
                worker.id
            )));
        }
        if worker.timeout_secs == 0 || worker.timeout_secs > 1800 {
            return Err(ConfigError::Invalid(format!(
                "agent_task worker '{}' timeout_secs must be 1..=1800",
                worker.id
            )));
        }
        if worker.permission_profile.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "agent_task worker '{}' permission_profile must not be empty",
                worker.id
            )));
        }
        if worker.env_allowlist.iter().any(|name| {
            name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        }) {
            return Err(ConfigError::Invalid(format!(
                "agent_task worker '{}' env_allowlist contains an invalid name",
                worker.id
            )));
        }
    }
    if config.enabled && config.workers.is_empty() {
        return Err(ConfigError::Invalid(
            "agent_task.enabled=true requires at least one worker".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RouterConfig {
    pub profile: String,
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
    pub file_write: FileWriteConfig,
    pub explore: ExploreLimitsConfig,
}

/// `list_dir` / `grep` の探索上限（timeout 前のメモリ・I/O 抑制）。
#[derive(Debug, Clone)]
pub struct ExploreLimitsConfig {
    pub max_list_entries: usize,
    pub max_grep_files_scanned: usize,
    pub max_grep_matches: usize,
    pub max_grep_file_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellExecApprovalMode {
    /// 実行前承認を要求しない（即拒否）。
    Never,
    /// 実行直前にクライアントへ yes/no を求める（既定）。
    #[default]
    Ask,
    /// 承認 UI なしで実行する。
    Always,
}

impl ShellExecApprovalMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "never" => Some(Self::Never),
            "ask" => Some(Self::Ask),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Ask => "ask",
            Self::Always => "always",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellExecConfig {
    pub enabled: bool,
    pub allowed_commands: Vec<String>,
    pub approval: ShellExecApprovalMode,
    pub auto_approve_patterns: ShellExecAutoApprovePatterns,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellExecAutoApprovePatterns {
    pub read_only: Vec<String>,
    pub mutating: Vec<String>,
}

impl Default for ShellExecConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_commands: vec![],
            approval: ShellExecApprovalMode::Ask,
            auto_approve_patterns: ShellExecAutoApprovePatterns::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReadFileConfig {
    pub allowed_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileWriteApprovalMode {
    Never,
    #[default]
    Ask,
    Always,
}

impl FileWriteApprovalMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "never" => Some(Self::Never),
            "ask" => Some(Self::Ask),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Ask => "ask",
            Self::Always => "always",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileWriteConfig {
    pub enabled: bool,
    pub allowed_roots: Vec<PathBuf>,
    pub approval: FileWriteApprovalMode,
    pub max_file_bytes: usize,
    pub max_patch_bytes: usize,
    pub max_preview_bytes: usize,
    pub journal_retention_days: u32,
    pub journal_max_bytes: u64,
}

impl Default for FileWriteConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_roots: vec![PathBuf::from(".")],
            approval: FileWriteApprovalMode::Ask,
            max_file_bytes: DEFAULT_MAX_FILE_WRITE_BYTES,
            max_patch_bytes: DEFAULT_MAX_PATCH_BYTES,
            max_preview_bytes: DEFAULT_MAX_PREVIEW_BYTES,
            journal_retention_days: DEFAULT_JOURNAL_RETENTION_DAYS,
            journal_max_bytes: DEFAULT_JOURNAL_MAX_BYTES,
        }
    }
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
                approval: ShellExecApprovalMode::Ask,
                auto_approve_patterns: ShellExecAutoApprovePatterns::default(),
            },
            read_file: ReadFileConfig {
                allowed_roots: vec![PathBuf::from(".")],
            },
            file_write: FileWriteConfig::default(),
            explore: ExploreLimitsConfig::default(),
        }
    }
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            profile: "default".to_string(),
        }
    }
}

pub fn default_conversation_store_root_with_home(home: impl AsRef<Path>) -> PathBuf {
    home.as_ref()
        .join(".local/share/aibe")
        .join("conversations")
}

impl Default for ExploreLimitsConfig {
    fn default() -> Self {
        Self {
            max_list_entries: DEFAULT_MAX_LIST_ENTRIES,
            max_grep_files_scanned: DEFAULT_MAX_GREP_FILES_SCANNED,
            max_grep_matches: DEFAULT_MAX_GREP_MATCHES,
            max_grep_file_bytes: DEFAULT_MAX_GREP_FILE_BYTES,
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
#[derive(Clone)]
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

/// `external_commands.command` が `shell_exec` allowlist に含まれることを検証する。
pub fn validate_external_commands(
    external_commands: &[ExternalCommandConfig],
    allowed_commands: &[String],
) -> Result<(), ConfigError> {
    let mut seen_names = std::collections::HashSet::new();
    for entry in external_commands {
        if entry.name.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "[[external_commands]] name must not be empty".into(),
            ));
        }
        if !seen_names.insert(entry.name.clone()) {
            return Err(ConfigError::Invalid(format!(
                "duplicate external_commands name: {}",
                entry.name
            )));
        }
        if entry.command.trim().is_empty() {
            return Err(ConfigError::Invalid(format!(
                "external_commands '{}' command must not be empty",
                entry.name
            )));
        }
        if !allowed_commands.iter().any(|c| c == &entry.command) {
            return Err(ConfigError::Invalid(format!(
                "external_commands '{}' command '{}' is not in tools.shell_exec.allowed_commands",
                entry.name, entry.command
            )));
        }
    }
    Ok(())
}

/// 設定の読み込み。
pub trait ConfigLoader {
    fn load(&self) -> Result<AppConfig, ConfigError>;
}

#[cfg(test)]
mod config_tests {
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

    #[test]
    fn resolve_feature_pack_generic_memory_with_none_feature_files_is_empty() {
        let cfg = MemoryConfig {
            enabled: true,
            kind_files: Some(vec![]),
            recipe_files: Some(vec![]),
            feature_files: None,
        };
        let pack = cfg.resolve_feature_pack();
        assert_eq!(pack.mode, crate::domain::EffectiveFeatureMode::Empty);
        assert!(pack.config.feature_files.is_empty());
    }

    #[test]
    fn resolve_feature_pack_default_is_baseline_compat() {
        let pack = MemoryConfig::default().resolve_feature_pack();
        assert_eq!(
            pack.mode,
            crate::domain::EffectiveFeatureMode::BaselineCompat
        );
        assert!(pack.config.feature_files.is_empty());
    }

    #[test]
    fn resolve_feature_pack_explicit_empty_feature_files_is_empty() {
        let cfg = MemoryConfig {
            enabled: true,
            kind_files: None,
            recipe_files: None,
            feature_files: Some(vec![]),
        };
        let pack = cfg.resolve_feature_pack();
        assert_eq!(pack.mode, crate::domain::EffectiveFeatureMode::Empty);
        assert!(pack.config.feature_files.is_empty());
    }
}
