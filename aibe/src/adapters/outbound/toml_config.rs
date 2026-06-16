//! `~/.config/aibe/config.toml` アダプタ。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use toml::Value;

use crate::ports::outbound::{
    default_conversation_store_root_with_home, validate_external_commands, AppConfig, ConfigError,
    ConfigLoader, ExternalCommandConfig, LlmBackend, LlmGenerationParams, LlmProfile,
    LlmProfilesConfig, LlmProviderKind, MemoryConfig, RouterConfig, ToolsConfig,
    DEFAULT_EXTERNAL_COMMAND_TIMEOUT_SECS,
};

const DEFAULT_CONFIG_PATH: &str = ".config/aibe/config.toml";

/// TOML + 環境変数オーバーライド。
pub struct TomlConfig {
    path: PathBuf,
}

impl TomlConfig {
    pub fn load() -> Result<AppConfig, ConfigError> {
        Self::from_path(Self::resolve_path()).load()
    }

    pub fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    fn resolve_path() -> PathBuf {
        if let Ok(p) = std::env::var("AIBE_CONFIG") {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_CONFIG_PATH)
    }
}

impl ConfigLoader for TomlConfig {
    fn load(&self) -> Result<AppConfig, ConfigError> {
        let (file_cfg, root) = if self.path.is_file() {
            let raw = fs::read_to_string(&self.path)
                .map_err(|e| ConfigError::Io(format!("{}: {e}", self.path.display())))?;
            let file_cfg: FileConfig =
                toml::from_str(&raw).map_err(|e| ConfigError::Invalid(e.to_string()))?;
            let root: toml::Table =
                toml::from_str(&raw).map_err(|e| ConfigError::Invalid(e.to_string()))?;
            (Some(file_cfg), Some(root))
        } else {
            (None, None)
        };

        let socket_path = std::env::var("AIBE_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                file_cfg
                    .as_ref()
                    .and_then(|c| c.socket_path.clone())
                    .map(expand_home)
                    .unwrap_or_else(aibe_client::default_socket_path)
            });

        let llm = parse_llm_profiles(root.as_ref(), file_cfg.as_ref())?;
        let tools = parse_tools(file_cfg.as_ref())?;
        let external_commands = parse_external_commands(file_cfg.as_ref());
        validate_external_commands(&external_commands, &tools.shell_exec.allowed_commands)?;
        let memory = parse_memory(file_cfg.as_ref(), &self.path);
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        Ok(AppConfig {
            socket_path,
            conversation_store_root: default_conversation_store_root_with_home(&home),
            router: parse_router(file_cfg.as_ref()),
            llm,
            tools,
            external_commands,
            memory,
        })
    }
}

fn parse_router(file: Option<&FileConfig>) -> RouterConfig {
    let fallback = file
        .and_then(|c| c.default_profile.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string());
    RouterConfig {
        profile: file
            .and_then(|c| c.router.as_ref())
            .and_then(|router| router.profile.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback),
    }
}

fn parse_llm_profiles(
    root: Option<&toml::Table>,
    file: Option<&FileConfig>,
) -> Result<LlmProfilesConfig, ConfigError> {
    let Some(root) = root else {
        return Ok(LlmProfilesConfig::default_mock());
    };

    let llm_value = root.get("llm");
    let profiles_value = root.get("profiles");

    if llm_value.is_none() && profiles_value.is_none() {
        return Ok(LlmProfilesConfig::default_mock());
    }

    let default_profile = file
        .and_then(|f| f.default_profile.clone())
        .unwrap_or_else(|| "default".to_string());

    match classify_llm_section(llm_value)? {
        LlmSectionKind::Legacy(flat) => parse_legacy_llm(flat, profiles_value, default_profile),
        LlmSectionKind::Named(backends) => {
            parse_named_llm(backends, profiles_value, default_profile)
        }
        LlmSectionKind::Absent => {
            if profiles_value.is_some() {
                return Err(ConfigError::Invalid(
                    "[profiles] requires at least one [llm.<name>] backend".into(),
                ));
            }
            Ok(LlmProfilesConfig::default_mock())
        }
    }
}

enum LlmSectionKind {
    Legacy(toml::Table),
    Named(HashMap<String, toml::Table>),
    Absent,
}

fn classify_llm_section(llm: Option<&Value>) -> Result<LlmSectionKind, ConfigError> {
    let Some(Value::Table(table)) = llm else {
        return Ok(LlmSectionKind::Absent);
    };

    let mut named = HashMap::new();
    let mut flat = toml::Table::new();

    for (key, value) in table {
        if value.is_table() {
            named.insert(key.clone(), value.as_table().cloned().expect("is_table"));
        } else {
            flat.insert(key.clone(), value.clone());
        }
    }

    if !named.is_empty() {
        if flat.contains_key("provider")
            || flat.contains_key("api_key")
            || flat.contains_key("base_url")
            || flat.contains_key("model")
        {
            return Err(ConfigError::Invalid(
                "cannot mix flat [llm] keys with [llm.<name>] tables".into(),
            ));
        }
        return Ok(LlmSectionKind::Named(named));
    }

    if flat.is_empty() {
        return Ok(LlmSectionKind::Absent);
    }

    Ok(LlmSectionKind::Legacy(flat))
}

fn parse_legacy_llm(
    mut flat: toml::Table,
    profiles_value: Option<&Value>,
    default_profile: String,
) -> Result<LlmProfilesConfig, ConfigError> {
    if profiles_value.is_some() {
        return Err(ConfigError::Invalid(
            "legacy flat [llm] cannot be combined with [profiles]".into(),
        ));
    }

    let provider = flat
        .remove("provider")
        .and_then(|v| v.as_str().map(str::to_string))
        .or_else(|| std::env::var("AIBE_LLM_PROVIDER").ok())
        .unwrap_or_else(|| "mock".to_string());

    let model = flat
        .remove("model")
        .and_then(|v| v.as_str().map(str::to_string))
        .or_else(|| std::env::var("AIBE_MODEL").ok())
        .unwrap_or_else(|| default_model_for_provider(&provider));

    let backend = parse_backend_fields(&provider, &mut flat, true)?;

    let mut backends = HashMap::new();
    backends.insert("default".to_string(), backend);

    let mut profiles = HashMap::new();
    profiles.insert(
        "default".to_string(),
        LlmProfile {
            llm: "default".to_string(),
            model,
            params: LlmGenerationParams::default(),
        },
    );

    Ok(LlmProfilesConfig {
        backends,
        profiles,
        default_profile: if default_profile.is_empty() {
            "default".to_string()
        } else {
            default_profile
        },
    })
}

fn parse_named_llm(
    backend_tables: HashMap<String, toml::Table>,
    profiles_value: Option<&Value>,
    default_profile: String,
) -> Result<LlmProfilesConfig, ConfigError> {
    let mut backends = HashMap::new();
    for (name, mut table) in backend_tables {
        if table.contains_key("model") {
            return Err(ConfigError::Invalid(format!(
                "[llm.{name}] must not contain model (use [profiles.<name>] instead)"
            )));
        }
        let provider = table
            .remove("provider")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| ConfigError::Invalid(format!("[llm.{name}] requires provider")))?;
        let backend = parse_backend_fields(&provider, &mut table, false)?;
        backends.insert(name, backend);
    }

    let profiles = parse_profiles_table(profiles_value)?;
    if profiles.is_empty() {
        return Err(ConfigError::Invalid(
            "new format requires at least one [profiles.<name>]".into(),
        ));
    }

    Ok(LlmProfilesConfig {
        backends,
        profiles,
        default_profile,
    })
}

fn parse_profiles_table(
    profiles_value: Option<&Value>,
) -> Result<HashMap<String, LlmProfile>, ConfigError> {
    let Some(Value::Table(table)) = profiles_value else {
        return Err(ConfigError::Invalid(
            "new format requires [profiles.<name>] tables".into(),
        ));
    };

    let mut profiles = HashMap::new();
    for (name, value) in table {
        let inner = value
            .as_table()
            .ok_or_else(|| ConfigError::Invalid(format!("[profiles.{name}] must be a table")))?;
        let llm = get_string(inner, "llm", &format!("[profiles.{name}]"))?;
        let model = get_string(inner, "model", &format!("[profiles.{name}]"))?;
        let temperature = inner
            .get("temperature")
            .and_then(|v| v.as_float().map(|f| f as f32));
        let max_output_tokens =
            parse_optional_u32(inner, "max_output_tokens", &format!("[profiles.{name}]"))?;

        profiles.insert(
            name.clone(),
            LlmProfile {
                llm,
                model,
                params: LlmGenerationParams {
                    temperature,
                    max_output_tokens,
                },
            },
        );
    }
    Ok(profiles)
}

fn parse_backend_fields(
    provider: &str,
    table: &mut toml::Table,
    allow_env: bool,
) -> Result<LlmBackend, ConfigError> {
    let kind = parse_provider_kind(provider)?;

    match kind {
        LlmProviderKind::Mock => Ok(LlmBackend {
            provider: kind,
            api_key: String::new(),
            base_url: String::new(),
        }),
        LlmProviderKind::OpenAiCompatible => {
            let api_key = table
                .remove("api_key")
                .and_then(|v| v.as_str().map(str::to_string))
                .or_else(|| {
                    if allow_env {
                        std::env::var("AIBE_API_KEY").ok()
                    } else {
                        None
                    }
                })
                .filter(|k| !k.is_empty())
                .ok_or_else(|| {
                    ConfigError::Invalid(
                        "openai_compatible requires api_key in [llm.<name>]".into(),
                    )
                })?;
            let base_url = table
                .remove("base_url")
                .and_then(|v| v.as_str().map(str::to_string))
                .or_else(|| {
                    if allow_env {
                        std::env::var("AIBE_BASE_URL").ok()
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            Ok(LlmBackend {
                provider: kind,
                api_key,
                base_url: base_url.trim_end_matches('/').to_string(),
            })
        }
        LlmProviderKind::Gemini => {
            let api_key = table
                .remove("api_key")
                .and_then(|v| v.as_str().map(str::to_string))
                .or_else(|| {
                    if allow_env {
                        std::env::var("AIBE_API_KEY").ok()
                    } else {
                        None
                    }
                })
                .filter(|k| !k.is_empty())
                .ok_or_else(|| {
                    ConfigError::Invalid("gemini requires api_key in [llm.<name>]".into())
                })?;
            let base_url = table
                .remove("base_url")
                .and_then(|v| v.as_str().map(str::to_string))
                .or_else(|| {
                    if allow_env {
                        std::env::var("AIBE_BASE_URL").ok()
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string());
            Ok(LlmBackend {
                provider: kind,
                api_key,
                base_url: base_url.trim_end_matches('/').to_string(),
            })
        }
    }
}

fn parse_provider_kind(provider: &str) -> Result<LlmProviderKind, ConfigError> {
    match provider {
        "mock" => Ok(LlmProviderKind::Mock),
        "openai_compatible" | "openai-compatible" => Ok(LlmProviderKind::OpenAiCompatible),
        "gemini" => Ok(LlmProviderKind::Gemini),
        other => Err(ConfigError::Invalid(format!(
            "unknown llm provider: {other}"
        ))),
    }
}

fn default_model_for_provider(provider: &str) -> String {
    match provider {
        "gemini" => "gemini-3.5-flash".to_string(),
        "openai_compatible" | "openai-compatible" => "gpt-4o-mini".to_string(),
        _ => "mock".to_string(),
    }
}

fn get_string(table: &toml::Table, key: &str, ctx: &str) -> Result<String, ConfigError> {
    table
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| ConfigError::Invalid(format!("{ctx} requires {key}")))
}

/// TOML 整数を `u32` に変換。負値・`u32::MAX` 超過は読み込みエラー。
fn parse_optional_u32(
    table: &toml::Table,
    key: &str,
    ctx: &str,
) -> Result<Option<u32>, ConfigError> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    let Some(n) = value.as_integer() else {
        return Err(ConfigError::Invalid(format!(
            "{ctx} {key} must be an integer"
        )));
    };
    if n < 0 || n > u32::MAX as i64 {
        return Err(ConfigError::Invalid(format!(
            "{ctx} {key} must be between 0 and {}",
            u32::MAX
        )));
    }
    Ok(Some(n as u32))
}

fn parse_tools(file: Option<&FileConfig>) -> Result<ToolsConfig, ConfigError> {
    let section = file.and_then(|c| c.tools.as_ref());
    let mut tools = ToolsConfig::default();
    if let Some(t) = section {
        if let Some(n) = t.max_rounds {
            if n < crate::ports::outbound::MIN_MAX_TOOL_ROUNDS {
                return Err(ConfigError::Invalid(
                    "[tools] max_rounds must be at least 1".into(),
                ));
            }
            tools.max_rounds = n;
        }
        if let Some(ms) = t.exec_timeout_ms {
            tools.exec_timeout_ms = ms;
        }
        if let Some(n) = t.max_tool_output_bytes {
            tools.max_tool_output_bytes = n;
        }
        if let Some(shell) = t.shell_exec.as_ref() {
            if let Some(enabled) = shell.enabled {
                tools.shell_exec.enabled = enabled;
            }
            if let Some(cmds) = shell.allowed_commands.clone() {
                tools.shell_exec.allowed_commands = cmds;
            }
            if let Some(mode) = shell.shell_exec_approval.as_deref() {
                match crate::ports::outbound::ShellExecApprovalMode::parse(mode) {
                    Some(parsed) => tools.shell_exec.approval = parsed,
                    None => {
                        return Err(ConfigError::Invalid(format!(
                            "[tools.shell_exec] shell_exec_approval must be never, ask, or always (got {mode:?})"
                        )));
                    }
                }
            }
            if let Some(patterns) = shell.auto_approve_patterns.as_ref() {
                tools.shell_exec.auto_approve_patterns.read_only = patterns
                    .read_only
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect();
                tools.shell_exec.auto_approve_patterns.mutating = patterns
                    .mutating
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect();
            }
        }
        if let Some(rf) = t.read_file.as_ref() {
            if let Some(roots) = rf.allowed_roots.clone() {
                tools.read_file.allowed_roots = roots.into_iter().map(expand_home).collect();
            }
        }
        if let Some(s) = t.termination_strategy.as_deref() {
            if let Some(strategy) = crate::ports::outbound::TerminationStrategy::parse(s) {
                tools.termination_strategy = strategy;
            }
        }
        if let Some(e) = t.explore.as_ref() {
            if let Some(n) = e.max_list_entries {
                if n == 0 {
                    return Err(ConfigError::Invalid(
                        "[tools.explore] max_list_entries must be at least 1".into(),
                    ));
                }
                tools.explore.max_list_entries = n;
            }
            if let Some(n) = e.max_grep_files_scanned {
                if n == 0 {
                    return Err(ConfigError::Invalid(
                        "[tools.explore] max_grep_files_scanned must be at least 1".into(),
                    ));
                }
                tools.explore.max_grep_files_scanned = n;
            }
            if let Some(n) = e.max_grep_matches {
                if n == 0 {
                    return Err(ConfigError::Invalid(
                        "[tools.explore] max_grep_matches must be at least 1".into(),
                    ));
                }
                tools.explore.max_grep_matches = n;
            }
            if let Some(n) = e.max_grep_file_bytes {
                if n == 0 {
                    return Err(ConfigError::Invalid(
                        "[tools.explore] max_grep_file_bytes must be at least 1".into(),
                    ));
                }
                tools.explore.max_grep_file_bytes = n;
            }
        }
    }
    Ok(tools)
}

fn parse_external_commands(file: Option<&FileConfig>) -> Vec<ExternalCommandConfig> {
    file.and_then(|f| f.external_commands.as_ref())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|e| ExternalCommandConfig {
            name: e.name,
            description: e.description.unwrap_or_default(),
            command: e.command,
            args: e.args.unwrap_or_default(),
            timeout_secs: e
                .timeout_secs
                .unwrap_or(DEFAULT_EXTERNAL_COMMAND_TIMEOUT_SECS),
        })
        .collect()
}

fn expand_home(path: String) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

fn parse_memory(file: Option<&FileConfig>, config_path: &std::path::Path) -> MemoryConfig {
    let config_dir = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let config_dir = fs::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_path_buf());
    let mut memory = file
        .and_then(|c| c.memory.as_ref())
        .map(|section| parse_memory_section(section, &config_dir))
        .unwrap_or_default();
    if let Ok(raw) = std::env::var("AIBE_MEMORY_ENABLED") {
        if let Some(enabled) = parse_bool_env(&raw) {
            memory.enabled = enabled;
        }
    }
    memory
}

fn parse_memory_section(section: &MemorySection, config_dir: &std::path::Path) -> MemoryConfig {
    MemoryConfig {
        enabled: section.enabled.unwrap_or(true),
        kind_files: section.kind_files.as_ref().map(|paths| {
            paths
                .iter()
                .map(|p| resolve_memory_path(p, config_dir))
                .collect()
        }),
        recipe_files: section.recipe_files.as_ref().map(|paths| {
            paths
                .iter()
                .map(|p| resolve_memory_path(p, config_dir))
                .collect()
        }),
        feature_files: section.feature_files.as_ref().map(|paths| {
            paths
                .iter()
                .map(|p| resolve_memory_path(p, config_dir))
                .collect()
        }),
    }
}

fn resolve_memory_path(path: &str, config_dir: &std::path::Path) -> PathBuf {
    let expanded = expand_home(path.to_string());
    if expanded.is_absolute() {
        expanded
    } else {
        config_dir.join(expanded)
    }
}

fn parse_bool_env(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    socket_path: Option<String>,
    default_profile: Option<String>,
    router: Option<RouterSection>,
    #[serde(default)]
    external_commands: Option<Vec<ExternalCommandSection>>,
    tools: Option<ToolsSection>,
    memory: Option<MemorySection>,
}

#[derive(Debug, Deserialize)]
struct MemorySection {
    enabled: Option<bool>,
    #[serde(default)]
    kind_files: Option<Vec<String>>,
    #[serde(default)]
    recipe_files: Option<Vec<String>>,
    #[serde(default)]
    feature_files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RouterSection {
    profile: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExternalCommandSection {
    name: String,
    description: Option<String>,
    command: String,
    #[serde(default)]
    args: Option<Vec<String>>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ToolsSection {
    max_rounds: Option<u32>,
    exec_timeout_ms: Option<u64>,
    max_tool_output_bytes: Option<usize>,
    termination_strategy: Option<String>,
    shell_exec: Option<ShellExecSection>,
    read_file: Option<ReadFileSection>,
    explore: Option<ExploreSection>,
}

#[derive(Debug, Deserialize)]
struct ExploreSection {
    max_list_entries: Option<usize>,
    max_grep_files_scanned: Option<usize>,
    max_grep_matches: Option<usize>,
    max_grep_file_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ShellExecSection {
    enabled: Option<bool>,
    allowed_commands: Option<Vec<String>>,
    shell_exec_approval: Option<String>,
    auto_approve_patterns: Option<AutoApprovePatternsSection>,
}

#[derive(Debug, Deserialize)]
struct AutoApprovePatternsSection {
    #[serde(default)]
    read_only: Vec<String>,
    #[serde(default)]
    mutating: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReadFileSection {
    allowed_roots: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::TerminationStrategy;

    #[test]
    fn router_profile_falls_back_to_default_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_profile = "fast"
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(cfg.router.profile, "fast");
    }

    #[test]
    fn router_section_overrides_default_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_profile = "fast"

[router]
profile = "slow"
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(cfg.router.profile, "slow");
    }

    #[test]
    fn parses_termination_strategy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[tools]
termination_strategy = "conversation_replay"
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(
            cfg.tools.termination_strategy,
            TerminationStrategy::ConversationReplay
        );
    }

    #[test]
    fn memory_paths_are_resolved_against_config_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_dir = dir.path().join("nested");
        fs::create_dir_all(&config_dir).expect("mkdir");
        let path = config_dir.join("config.toml");
        fs::write(
            &path,
            r#"
[memory]
kind_files = ["memory/packs/aish-memory/kinds.toml"]
recipe_files = ["memory/packs/aish-memory/recipes/clarify-goal.toml"]
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        let kind_files = cfg.memory.kind_files.expect("kind_files");
        let recipe_files = cfg.memory.recipe_files.expect("recipe_files");
        assert!(kind_files[0].is_absolute());
        assert!(recipe_files[0].is_absolute());
        assert!(kind_files[0].ends_with("nested/memory/packs/aish-memory/kinds.toml"));
        assert!(
            recipe_files[0].ends_with("nested/memory/packs/aish-memory/recipes/clarify-goal.toml")
        );
    }

    #[test]
    fn rejects_max_rounds_zero_in_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[tools]
max_rounds = 0
"#,
        )
        .expect("write");

        let err = TomlConfig::from_path(path).load().unwrap_err();
        match err {
            ConfigError::Invalid(msg) => assert!(msg.contains("max_rounds")),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn legacy_flat_llm_backward_compat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[llm]
provider = "openai_compatible"
api_key = "test-key"
base_url = "http://127.0.0.1:8080/v1"
model = "local"
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(cfg.llm.default_profile, "default");
        let backend = cfg.llm.backends.get("default").expect("backend");
        assert_eq!(backend.provider, LlmProviderKind::OpenAiCompatible);
        assert_eq!(backend.base_url, "http://127.0.0.1:8080/v1");
        let profile = cfg.llm.profiles.get("default").expect("profile");
        assert_eq!(profile.model, "local");
    }

    #[test]
    fn parses_multi_backend_and_profiles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_profile = "fast"

[llm.gemini-studio]
provider = "gemini"
api_key = "k"

[llm.lmstudio]
provider = "openai_compatible"
api_key = "k"
base_url = "http://127.0.0.1:1/v1"

[profiles.fast]
llm = "gemini-studio"
model = "gemini-3.5-flash"

[profiles.local]
llm = "lmstudio"
model = "qwen"
temperature = 0.5
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(cfg.llm.default_profile, "fast");
        assert_eq!(cfg.llm.backends.len(), 2);
        assert_eq!(cfg.llm.profiles.len(), 2);
        let fast = cfg.llm.profiles.get("fast").expect("fast");
        assert_eq!(fast.llm, "gemini-studio");
        assert_eq!(fast.params.temperature, None);
        let local = cfg.llm.profiles.get("local").expect("local");
        assert_eq!(local.params.temperature, Some(0.5));
    }

    #[test]
    fn rejects_model_in_llm_backend_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[llm.studio]
provider = "gemini"
api_key = "k"
model = "x"

[profiles.p]
llm = "studio"
model = "y"
"#,
        )
        .expect("write");

        let err = TomlConfig::from_path(path).load().unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn rejects_mixed_flat_and_named_llm() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[llm]
provider = "mock"

[llm.named]
provider = "mock"

[profiles.p]
llm = "named"
model = "m"
"#,
        )
        .expect("write");

        let err = TomlConfig::from_path(path).load().unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn rejects_new_format_without_profiles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[llm.studio]
provider = "mock"
"#,
        )
        .expect("write");

        let err = TomlConfig::from_path(path).load().unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn rejects_negative_max_output_tokens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[llm.mock]
provider = "mock"

[profiles.p]
llm = "mock"
model = "m"
max_output_tokens = -1
"#,
        )
        .expect("write");

        let err = TomlConfig::from_path(path).load().unwrap_err();
        match err {
            ConfigError::Invalid(msg) => assert!(msg.contains("max_output_tokens")),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn rejects_max_output_tokens_overflow() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let overflow = (u32::MAX as u64) + 1;
        fs::write(
            &path,
            format!(
                r#"
[llm.mock]
provider = "mock"

[profiles.p]
llm = "mock"
model = "m"
max_output_tokens = {overflow}
"#
            ),
        )
        .expect("write");

        let err = TomlConfig::from_path(path).load().unwrap_err();
        match err {
            ConfigError::Invalid(msg) => assert!(msg.contains("max_output_tokens")),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn env_ignored_for_named_llm_backends() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[llm.studio]
provider = "mock"

[profiles.p]
llm = "studio"
model = "from-toml"
"#,
        )
        .expect("write");

        unsafe {
            std::env::set_var("AIBE_MODEL", "from-env");
            std::env::set_var("AIBE_API_KEY", "env-key-should-not-apply");
        }
        let cfg = TomlConfig::from_path(path).load().expect("load");
        unsafe {
            std::env::remove_var("AIBE_MODEL");
            std::env::remove_var("AIBE_API_KEY");
        }

        let profile = cfg.llm.profiles.get("p").expect("profile");
        assert_eq!(profile.model, "from-toml");
        let backend = cfg.llm.backends.get("studio").expect("backend");
        assert!(backend.api_key.is_empty());
    }

    #[test]
    fn rejects_missing_default_profile_name() {
        use crate::adapters::outbound::build_profile_registry;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_profile = "no-such"

[llm.mock]
provider = "mock"

[profiles.fast]
llm = "mock"
model = "m"
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        let err = match build_profile_registry(&cfg.llm) {
            Err(e) => e,
            Ok(_) => panic!("expected registry build to fail"),
        };
        match err {
            ConfigError::Invalid(msg) => assert!(msg.contains("default_profile")),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn accepts_max_output_tokens_at_u32_max() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            format!(
                r#"
[llm.mock]
provider = "mock"

[profiles.p]
llm = "mock"
model = "m"
max_output_tokens = {}
"#,
                u32::MAX
            ),
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        let profile = cfg.llm.profiles.get("p").expect("profile");
        assert_eq!(profile.params.max_output_tokens, Some(u32::MAX));
    }

    #[test]
    fn parses_external_commands_with_default_timeout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[tools.shell_exec]
allowed_commands = ["codex"]

[[external_commands]]
name = "codex"
description = "Codex CLI"
command = "codex"
args = ["exec", "{prompt}"]
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert_eq!(cfg.external_commands.len(), 1);
        assert_eq!(cfg.external_commands[0].name, "codex");
        assert_eq!(
            cfg.external_commands[0].timeout_secs,
            DEFAULT_EXTERNAL_COMMAND_TIMEOUT_SECS
        );
    }

    #[test]
    fn parses_memory_enabled_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[memory]
enabled = false
"#,
        )
        .expect("write");

        let cfg = TomlConfig::from_path(path).load().expect("load");
        assert!(!cfg.memory.enabled);
    }

    #[test]
    fn env_memory_enabled_overrides_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[memory]
enabled = true
"#,
        )
        .expect("write");

        unsafe {
            std::env::set_var("AIBE_MEMORY_ENABLED", "0");
        }
        let cfg = TomlConfig::from_path(path).load().expect("load");
        unsafe {
            std::env::remove_var("AIBE_MEMORY_ENABLED");
        }
        assert!(!cfg.memory.enabled);
    }

    #[test]
    fn rejects_external_command_not_in_allowlist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[tools.shell_exec]
allowed_commands = ["echo"]

[[external_commands]]
name = "codex"
command = "codex"
"#,
        )
        .expect("write");

        let err = TomlConfig::from_path(path).load().unwrap_err();
        match err {
            ConfigError::Invalid(msg) => {
                assert!(msg.contains("allowed_commands"));
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }
}
