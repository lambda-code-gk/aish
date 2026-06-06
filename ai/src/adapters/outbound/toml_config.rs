//! `~/.config/ai/config.toml` アダプタ。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use aibe_client::default_socket_path;
use serde::Deserialize;

use crate::domain::{tokens_from_config_value, AskToolsConfigRaw, ConfigToolsTokens};

pub const DEFAULT_HISTORY_MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub socket_path: PathBuf,
    pub ask_tools: ConfigToolsTokens,
    pub ask_default_profile: Option<String>,
    pub ask_filter: Option<String>,
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
}

const DEFAULT_CONFIG: &str = ".config/ai/config.toml";
const DEFAULT_HISTORY_DIR: &str = ".local/share/ai/history";

impl AiConfig {
    pub fn load() -> Self {
        let path = Self::resolve_path();
        let mut cfg = Self {
            socket_path: default_socket_path(),
            ask_tools: ConfigToolsTokens::default(),
            ask_default_profile: None,
            ask_filter: None,
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
                    if let Some(ask) = file.ask {
                        if let Some(tools) = ask.tools {
                            cfg.ask_tools = tokens_from_config_value(match tools {
                                AskToolsToml::String(s) => AskToolsConfigRaw::String(s),
                                AskToolsToml::Array(a) => AskToolsConfigRaw::Array(a),
                            });
                        }
                        cfg.ask_default_profile = ask.default_profile;
                        cfg.ask_filter = ask.filter.filter(|s| !s.is_empty());
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
        cfg
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
}

fn expand_home(path: String) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    socket_path: Option<String>,
    ask: Option<AskSection>,
    history_dir: Option<String>,
    history_max_entries: Option<usize>,
    log_tail_bytes: Option<usize>,
    presets: Option<HashMap<String, PresetToml>>,
}

#[derive(Debug, Deserialize)]
struct AskSection {
    tools: Option<AskToolsToml>,
    default_profile: Option<String>,
    filter: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresetToml {
    tools: Option<AskToolsToml>,
    profile: Option<String>,
    filter: Option<String>,
    log_tail_bytes: Option<usize>,
    quiet: Option<bool>,
    shell_exec_approval: Option<String>,
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
