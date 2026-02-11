//! reviewed ファイルによるセッション履歴の読み込み
//!
//! ファイル名規則: `reviewed_<ID>_user.txt` / `reviewed_<ID>_assistant.txt`
//! leakscan 通過分のみがここにあり、履歴はこのファイルのみから構築する。

use crate::domain::History;
use crate::ports::outbound::SessionHistoryLoader;
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::FileSystem;
use common::safe_session_path::REVIEWED_DIR;
use std::path::PathBuf;
use std::sync::Arc;

/// reviewed ファイル形式でセッション履歴を読み込むアダプタ（leakscan 有効時用）
#[cfg_attr(not(test), allow(dead_code))]
pub struct ReviewedSessionStorage {
    fs: Arc<dyn FileSystem>,
}

impl ReviewedSessionStorage {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }
}

impl SessionHistoryLoader for ReviewedSessionStorage {
    fn load(&self, session_dir: &SessionDir) -> Result<History, Error> {
        if !self.fs.exists(session_dir.as_ref()) {
            return Ok(History::new());
        }
        if self
            .fs
            .metadata(session_dir.as_ref())
            .map(|m| !m.is_dir())
            .unwrap_or(true)
        {
            return Ok(History::new());
        }
        let reviewed_dir = session_dir.as_ref().join(REVIEWED_DIR);
        if !self.fs.exists(&reviewed_dir) {
            return Ok(History::new());
        }
        let mut reviewed_files: Vec<PathBuf> = self
            .fs
            .read_dir(&reviewed_dir)?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |s| {
                        s.starts_with("reviewed_")
                            && (s.ends_with("_user.txt") || s.ends_with("_assistant.txt"))
                    })
                    && self.fs.metadata(path).map(|m| m.is_file()).unwrap_or(false)
            })
            .collect();
        reviewed_files.sort();

        let mut history = History::new();
        for reviewed_file in reviewed_files {
            match self.fs.read_to_string(&reviewed_file) {
                Ok(content) => {
                    if let Some(name_str) = reviewed_file.file_name().and_then(|n| n.to_str()) {
                        if name_str.ends_with("_user.txt") {
                            history.push_user(content);
                        } else if name_str.ends_with("_assistant.txt") {
                            history.push_assistant(content);
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to read reviewed file '{}': {}",
                        reviewed_file.display(),
                        e
                    );
                }
            }
        }
        Ok(history)
    }
}
