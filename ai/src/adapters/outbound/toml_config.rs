//! `~/.config/ai/config.toml` アダプタ。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use aibe_client::default_socket_path;
use serde::Deserialize;

use crate::domain::{tokens_from_config_value, AskToolsConfigRaw, ConfigToolsTokens};

pub const DEFAULT_HISTORY_MAX_ENTRIES: usize = 500;

pub const MEMORY_DISABLED_MESSAGE: &str =
    "contextual memory is disabled ([memory] enabled = false in ai config)";

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub socket_path: PathBuf,
    pub context_current: Option<String>,
    pub memory_enabled: bool,
    pub ask_tools: ConfigToolsTokens,
    pub ask_default_profile: Option<String>,
    pub ask_filter: Option<String>,
    pub ask_console_hints: Option<bool>,
    pub ask_progress: Option<bool>,
    pub history_dir: PathBuf,
    pub history_max_entries: usize,
    pub log_tail_bytes: Option<usize>,
    pub presets: HashMap<String, AiPresetConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct AiPresetConfig {
    pub tools: Option<ConfigToolsTokens>,
    pub profile: Option<String>,
    pub filter: Option<String>,
    pub log_tail_bytes: Option<usize>,
    pub quiet: Option<bool>,
    pub shell_exec_approval: Option<String>,
    pub console_hints: Option<bool>,
    pub progress: Option<bool>,
}

const DEFAULT_CONFIG: &str = ".config/ai/config.toml";
const DEFAULT_HISTORY_DIR: &str = ".local/share/ai/history";

impl AiConfig {
    pub fn load() -> Self {
        let path = Self::resolve_path();
        let mut cfg = Self {
            socket_path: default_socket_path(),
            context_current: None,
            memory_enabled: true,
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
            ask_console_hints: None,
            ask_progress: None,
            history_dir: Self::default_history_dir(),
            history_max_entries: DEFAULT_HISTORY_MAX_ENTRIES,
            log_tail_bytes: None,
            presets: HashMap::new(),
        };
        if path.is_file() {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(file) = toml::from_str::<FileConfig>(&raw) {
                    if let Some(p) = file.socket_path {
                        cfg.socket_path = expand_home(p);
                    }
                    if let Some(ctx) = file.context {
                        cfg.context_current = ctx.current.filter(|s| !s.is_empty());
                    }
                    if let Some(memory) = file.memory {
                        if let Some(enabled) = memory.enabled {
                            cfg.memory_enabled = enabled;
                        }
                    }
                    if let Some(ask) = file.ask {
                        if let Some(tools) = ask.tools {
                            cfg.ask_tools = tokens_from_config_value(match tools {
                                AskToolsToml::String(s) => AskToolsConfigRaw::String(s),
                                AskToolsToml::Array(a) => AskToolsConfigRaw::Array(a),
                            });
                        }
                        cfg.ask_default_profile = ask.default_profile;
                        cfg.ask_filter = ask.filter.filter(|s| !s.is_empty());
                        cfg.ask_console_hints = ask.console_hints;
                        cfg.ask_progress = ask.progress;
                    }
                    if let Some(history_dir) = file.history_dir {
                        cfg.history_dir = expand_home(history_dir);
                    }
                    if let Some(max_entries) = file.history_max_entries {
                        cfg.history_max_entries = max_entries;
                    }
                    cfg.log_tail_bytes = file.log_tail_bytes;
                    if let Some(presets) = file.presets {
                        cfg.presets = presets
                            .into_iter()
                            .map(|(name, preset)| (name, preset.into()))
                            .collect();
                    }
                }
            }
        }
        if let Ok(p) = std::env::var("AIBE_SOCKET_PATH") {
            cfg.socket_path = PathBuf::from(p);
        }
        if let Ok(raw) = std::env::var("AI_MEMORY_ENABLED") {
            if let Some(enabled) = parse_bool_env(&raw) {
                cfg.memory_enabled = enabled;
            }
        }
        cfg
    }

    pub fn ensure_memory_enabled(&self) -> Result<(), String> {
        if self.memory_enabled {
            Ok(())
        } else {
            Err(MEMORY_DISABLED_MESSAGE.to_string())
        }
    }

    fn resolve_path() -> PathBuf {
        if let Ok(p) = std::env::var("AI_CONFIG") {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_CONFIG)
    }

    fn default_history_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(DEFAULT_HISTORY_DIR)
    }

    pub fn save_context_current(name: &str) -> Result<(), String> {
        aibe_protocol::is_valid_memory_space_id(name)
            .then_some(())
            .ok_or_else(|| format!("invalid context name: {name}"))?;
        let path = Self::resolve_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut doc = if path.is_file() {
            let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            // parse 失敗時に既存 config を空で上書きしないため、エラーで止める
            toml::from_str::<toml::Value>(&raw).map_err(|e| {
                format!(
                    "existing config is not valid TOML ({}): {e}",
                    path.display()
                )
            })?
        } else {
            toml::Value::Table(Default::default())
        };
        let toml::Value::Table(ref mut root) = doc else {
            return Err("config root must be a table".into());
        };
        let ctx = root
            .entry("context")
            .or_insert_with(|| toml::Value::Table(Default::default()));
        let toml::Value::Table(ctx_table) = ctx else {
            return Err("config [context] must be a table".into());
        };
        ctx_table.insert("current".to_string(), toml::Value::String(name.to_string()));
        let out = toml::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        fs::write(&path, out).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn expand_home(path: String) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
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
    context: Option<ContextSection>,
    memory: Option<MemorySection>,
    ask: Option<AskSection>,
    history_dir: Option<String>,
    history_max_entries: Option<usize>,
    log_tail_bytes: Option<usize>,
    presets: Option<HashMap<String, PresetToml>>,
}

#[derive(Debug, Deserialize)]
struct MemorySection {
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ContextSection {
    current: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AskSection {
    tools: Option<AskToolsToml>,
    default_profile: Option<String>,
    filter: Option<String>,
    console_hints: Option<bool>,
    progress: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PresetToml {
    tools: Option<AskToolsToml>,
    profile: Option<String>,
    filter: Option<String>,
    log_tail_bytes: Option<usize>,
    quiet: Option<bool>,
    shell_exec_approval: Option<String>,
    console_hints: Option<bool>,
    progress: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AskToolsToml {
    String(String),
    Array(Vec<String>),
}

impl From<PresetToml> for AiPresetConfig {
    fn from(value: PresetToml) -> Self {
        Self {
            tools: value.tools.map(|tools| match tools {
                AskToolsToml::String(s) => tokens_from_config_value(AskToolsConfigRaw::String(s)),
                AskToolsToml::Array(a) => tokens_from_config_value(AskToolsConfigRaw::Array(a)),
            }),
            profile: value.profile.filter(|s| !s.is_empty()),
            filter: value.filter.filter(|s| !s.is_empty()),
            log_tail_bytes: value.log_tail_bytes,
            quiet: value.quiet,
            shell_exec_approval: value.shell_exec_approval.filter(|s| !s.is_empty()),
            console_hints: value.console_hints,
            progress: value.progress,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    use aibe_protocol::ToolName;

    use crate::domain::{resolve_output_filter, resolve_tools, AskToolsConfigRaw};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn parses_ask_tools_string_and_array() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(
            f,
            r#"
socket_path = "/tmp/s.sock"
[ask]
tools = "@read-only,shell_exec"
"#
        )
        .expect("write");

        let raw = fs::read_to_string(&path).expect("read");
        let file: FileConfig = toml::from_str(&raw).expect("parse");
        let ask = file.ask.expect("ask");
        let tokens =
            tokens_from_config_value(AskToolsConfigRaw::String(match ask.tools.expect("tools") {
                AskToolsToml::String(s) => s,
                AskToolsToml::Array(_) => panic!("expected string"),
            }));
        let resolved = resolve_tools(None, &tokens).expect("resolve");
        assert_eq!(
            resolved.allowlist.names(),
            &[
                ToolName::read_file(),
                ToolName::list_dir(),
                ToolName::grep(),
                ToolName::git_diff(),
                ToolName::git_status(),
                ToolName::shell_exec()
            ]
        );
    }

    #[test]
    fn load_config_file_cli_none_overrides_ask_tools() {
        let _guard = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(
            f,
            r#"
[ask]
tools = "@read-only"
"#
        )
        .expect("write");

        unsafe {
            std::env::set_var("AI_CONFIG", &path);
        }
        let cfg = AiConfig::load();
        unsafe {
            std::env::remove_var("AI_CONFIG");
        }

        let resolved = resolve_tools(Some("none"), &cfg.ask_tools).expect("resolve");
        assert!(resolved.allowlist.is_empty());

        let from_config_only = resolve_tools(None, &cfg.ask_tools).expect("resolve");
        assert_eq!(
            from_config_only.allowlist.names(),
            &[
                ToolName::read_file(),
                ToolName::list_dir(),
                ToolName::grep(),
                ToolName::git_diff(),
                ToolName::git_status()
            ]
        );
    }

    #[test]
    fn load_ask_filter_from_config() {
        let _guard = env_lock().lock().expect("env lock");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(
            f,
            r#"
[ask]
filter = "cat -n"
"#
        )
        .expect("write");

        unsafe {
            std::env::set_var("AI_CONFIG", &path);
            std::env::remove_var("AI_FILTER");
        }
        let cfg = AiConfig::load();
        unsafe {
            std::env::remove_var("AI_CONFIG");
        }
        assert_eq!(cfg.ask_filter.as_deref(), Some("cat -n"));
        assert_eq!(
            resolve_output_filter(None, cfg.ask_filter.as_deref()),
            Some("cat -n".into())
        );
    }

    #[test]
    fn load_presets_history_and_log_tail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(
            f,
            r#"
history_dir = "~/custom/history"
log_tail_bytes = 4096
[presets.fast]
tools = "@read-only"
profile = "fast"
filter = "cat"
log_tail_bytes = 1024
quiet = true
shell_exec_approval = "never"
"#
        )
        .expect("write");

        unsafe {
            std::env::set_var("AI_CONFIG", &path);
        }
        let cfg = AiConfig::load();
        unsafe {
            std::env::remove_var("AI_CONFIG");
        }

        assert!(cfg.history_dir.ends_with("custom/history"));
        assert_eq!(cfg.log_tail_bytes, Some(4096));
        let preset = cfg.presets.get("fast").expect("preset");
        assert_eq!(preset.profile.as_deref(), Some("fast"));
        assert_eq!(preset.filter.as_deref(), Some("cat"));
        assert_eq!(preset.log_tail_bytes, Some(1024));
        assert_eq!(preset.quiet, Some(true));
        assert_eq!(preset.shell_exec_approval.as_deref(), Some("never"));
        assert!(preset
            .tools
            .as_ref()
            .expect("tools")
            .0
            .contains(&"@read-only".into()));
    }

    #[test]
    fn load_console_hints_from_ask_and_preset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).expect("create");
        writeln!(
            f,
            r#"
[ask]
console_hints = false
[presets.script]
console_hints = true
"#
        )
        .expect("write");

        unsafe {
            std::env::set_var("AI_CONFIG", &path);
        }
        let cfg = AiConfig::load();
        unsafe {
            std::env::remove_var("AI_CONFIG");
        }

        assert_eq!(cfg.ask_console_hints, Some(false));
        assert_eq!(
            cfg.presets.get("script").and_then(|p| p.console_hints),
            Some(true)
        );
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
            std::env::set_var("AI_CONFIG", &path);
            std::env::set_var("AI_MEMORY_ENABLED", "0");
        }
        let cfg = AiConfig::load();
        unsafe {
            std::env::remove_var("AI_CONFIG");
            std::env::remove_var("AI_MEMORY_ENABLED");
        }
        assert!(!cfg.memory_enabled);
    }

    #[test]
    fn env_filter_overrides_config_filter() {
        unsafe {
            std::env::set_var("AI_FILTER", "sed 's/a/b/'");
        }
        assert_eq!(
            resolve_output_filter(std::env::var("AI_FILTER").ok(), Some("cat -n")),
            Some("sed 's/a/b/'".into())
        );
        unsafe {
            std::env::remove_var("AI_FILTER");
        }
    }
}
