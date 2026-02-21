//! Part ファイルへのセッション応答の保存
//!
//! ファイル名規則: `part_<ID>_user.txt` / `part_<ID>_assistant.txt`。
//! 履歴の読み込みは ManifestReviewedSessionStorage（manifest + reviewed）が行う。

use crate::ports::outbound::SessionResponseSaver;
use common::domain::SessionDir;
use common::error::Error;
use common::part_id::IdGenerator;
use common::ports::outbound::FileSystem;
use std::sync::Arc;

/// Part ファイル形式でセッション応答を保存するアダプタ（user/assistant の書き込みのみ）
pub struct PartSessionStorage {
    fs: Arc<dyn FileSystem>,
    id_gen: Arc<dyn IdGenerator>,
}

impl PartSessionStorage {
    pub fn new(fs: Arc<dyn FileSystem>, id_gen: Arc<dyn IdGenerator>) -> Self {
        Self { fs, id_gen }
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
