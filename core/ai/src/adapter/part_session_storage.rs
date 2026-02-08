//! Part ファイルによるセッション履歴の読み書き
//!
//! ファイル名規則: `part_<ID>_user.txt` / `part_<ID>_assistant.txt` をここに閉じ込める。

use crate::domain::History;
use crate::ports::outbound::{SessionHistoryLoader, SessionResponseSaver};
use common::domain::SessionDir;
use common::error::Error;
use common::part_id::IdGenerator;
use common::ports::outbound::FileSystem;
use std::path::PathBuf;
use std::sync::Arc;

/// Part ファイル形式でセッション履歴を読み書きするアダプタ
pub struct PartSessionStorage {
    fs: Arc<dyn FileSystem>,
    id_gen: Arc<dyn IdGenerator>,
}

impl PartSessionStorage {
    pub fn new(fs: Arc<dyn FileSystem>, id_gen: Arc<dyn IdGenerator>) -> Self {
        Self { fs, id_gen }
    }
}

impl SessionHistoryLoader for PartSessionStorage {
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
        let mut part_files: Vec<PathBuf> = self
            .fs
            .read_dir(session_dir.as_ref())?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |s| s.starts_with("part_"))
                    && self.fs.metadata(path).map(|m| m.is_file()).unwrap_or(false)
            })
            .collect();
        part_files.sort();

        let mut history = History::new();
        for part_file in part_files {
            match self.fs.read_to_string(&part_file) {
                Ok(content) => {
                    if let Some(name_str) = part_file.file_name().and_then(|n| n.to_str()) {
                        if name_str.ends_with("_user.txt") {
                            history.push_user(content);
                        } else if name_str.ends_with("_assistant.txt") {
                            history.push_assistant(content);
                        } else {
                            eprintln!("Warning: Unknown part file type: {}", name_str);
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to read part file '{}': {}",
                        part_file.display(),
                        e
                    );
                }
            }
        }
        Ok(history)
    }
}

impl SessionResponseSaver for PartSessionStorage {
    fn save_assistant(&self, session_dir: &SessionDir, response: &str) -> Result<(), Error> {
        if !self.fs.exists(session_dir.as_ref())
            || !self
                .fs
                .metadata(session_dir.as_ref())
                .map(|m| m.is_dir())
                .unwrap_or(false)
        {
            return Err(Error::io_msg("Session is not valid"));
        }
        let id = self.id_gen.next_id();
        let filename = format!("part_{}_assistant.txt", id);
        let file_path = session_dir.as_ref().join(&filename);
        self.fs.write(&file_path, response)
    }

    fn save_user(&self, session_dir: &SessionDir, content: &str) -> Result<(), Error> {
        if !self.fs.exists(session_dir.as_ref())
            || !self
                .fs
                .metadata(session_dir.as_ref())
                .map(|m| m.is_dir())
                .unwrap_or(false)
        {
            return Err(Error::io_msg("Session is not valid"));
        }
        let id = self.id_gen.next_id();
        let filename = format!("part_{}_user.txt", id);
        let file_path = session_dir.as_ref().join(&filename);
        let mut body = content.to_string();
        if !body.ends_with('\n') {
            body.push('\n');
        }
        self.fs.write(&file_path, &body)
    }
}
