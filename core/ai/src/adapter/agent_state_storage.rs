//! エージェント状態（続き用）を agent_state.json で保存・読み込みするアダプタ

use crate::ports::outbound::{AgentStateLoader, AgentStateSaver};
use common::domain::SessionDir;
use common::error::Error;
use common::msg::Msg;
use common::ports::outbound::FileSystem;
use std::sync::Arc;

const AGENT_STATE_FILENAME: &str = "agent_state.json";

/// セッション dir に agent_state.json で Vec<Msg> を保存する実装
pub struct FileAgentStateStorage {
    fs: Arc<dyn FileSystem>,
}

impl FileAgentStateStorage {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    fn path(session_dir: &SessionDir) -> std::path::PathBuf {
        session_dir.as_ref().join(AGENT_STATE_FILENAME)
    }
}

impl AgentStateSaver for FileAgentStateStorage {
    fn save(&self, session_dir: &SessionDir, messages: &[Msg]) -> Result<(), Error> {
        let path = Self::path(session_dir);
        let json = serde_json::to_string(messages).map_err(|e| Error::json(e.to_string()))?;
        self.fs.write(&path, &json)
    }

    fn clear(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let path = Self::path(session_dir);
        if self.fs.exists(&path) {
            self.fs.remove_file(&path)?;
        }
        Ok(())
    }
}

impl AgentStateLoader for FileAgentStateStorage {
    fn load(&self, session_dir: &SessionDir) -> Result<Option<Vec<Msg>>, Error> {
        let path = Self::path(session_dir);
        if !self.fs.exists(&path) {
            return Ok(None);
        }
        let s = self.fs.read_to_string(&path)?;
        let msgs: Vec<Msg> = serde_json::from_str(&s).map_err(|e| Error::json(e.to_string()))?;
        Ok(Some(msgs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;

    #[test]
    fn test_agent_state_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir_path = tmp.path().to_path_buf();
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(fs);
        let session_dir = SessionDir::new(dir_path.clone());

        let msgs = vec![
            Msg::user("hello"),
            Msg::assistant("hi"),
            Msg::tool_call("c1", "run_shell", serde_json::json!({"cmd": "ls"}), None),
            Msg::tool_result("c1", "run_shell", serde_json::json!({"ok": true})),
        ];
        storage.save(&session_dir, &msgs).unwrap();
        let loaded = storage.load(&session_dir).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap(), msgs);
    }

    #[test]
    fn test_agent_state_load_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(fs);
        let loaded = storage.load(&session_dir).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_agent_state_clear_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(fs);
        storage
            .save(&session_dir, &[Msg::user("x")])
            .unwrap();
        assert!(storage.load(&session_dir).unwrap().is_some());
        storage.clear(&session_dir).unwrap();
        assert!(storage.load(&session_dir).unwrap().is_none());
    }
}
