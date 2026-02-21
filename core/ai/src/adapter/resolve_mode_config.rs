//! モード設定読み込みの標準アダプタ（config/mode.d/<name>.json）

use std::sync::Arc;

use common::error::Error;
use common::ports::outbound::{EnvResolver, FileSystem};

use crate::domain::ModeConfig;
use crate::ports::outbound::ResolveModeConfig;

/// モード名に使える文字: 英数字・ハイフン・アンダースコアのみ（パストラバーサル防止）
fn is_valid_mode_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 標準実装（EnvResolver + FileSystem）
pub struct StdResolveModeConfig {
    env: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
}

impl StdResolveModeConfig {
    pub fn new(env: Arc<dyn EnvResolver>, fs: Arc<dyn FileSystem>) -> Self {
        Self { env, fs }
    }
}

impl ResolveModeConfig for StdResolveModeConfig {
    fn resolve(&self, mode_name: &str) -> Result<Option<ModeConfig>, Error> {
        if !is_valid_mode_name(mode_name) {
            return Err(Error::invalid_argument(format!(
                "Invalid mode name: '{}'. Use only letters, numbers, hyphen and underscore.",
                mode_name
            )));
        }
        // home を root として扱わず、EnvResolver::resolve_dirs() が返す config_dir 配下を参照する
        let dirs = self.env.resolve_dirs()?;
        let path = dirs
            .config_dir
            .join("mode.d")
            .join(format!("{}.json", mode_name));
        if !self.fs.exists(&path) {
            return Ok(None);
        }
        let contents = self
            .fs
            .read_to_string(&path)
            .map_err(|e| Error::io_msg(format!("{}: {}", path.display(), e)))?;
        ModeConfig::parse_json(&contents).map_err(|e| Error::json(format!("{}: {}", path.display(), e)))
            .map(Some)
    }

    fn list_names(&self) -> Result<Vec<String>, Error> {
        let dirs = self.env.resolve_dirs()?;
        let mode_d = dirs.config_dir.join("mode.d");
        if !self.fs.exists(&mode_d) {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for path in self.fs.read_dir(&mode_d)? {
            if !self.fs.metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            if is_valid_mode_name(stem) {
                names.push(stem.to_string());
            }
        }
        names.sort();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_mode_name() {
        assert!(is_valid_mode_name("plan"));
        assert!(is_valid_mode_name("agent"));
        assert!(is_valid_mode_name("my-mode"));
        assert!(is_valid_mode_name("mode_1"));
        assert!(!is_valid_mode_name(""));
        assert!(!is_valid_mode_name("../other"));
        assert!(!is_valid_mode_name("a/b"));
    }
}
