//! エージェント状態（続き用）を agent_state.json で保存・読み込みするアダプタ

use crate::ports::outbound::{AgentStateLoader, AgentStateSaver};
use common::domain::{PendingInput, SessionDir};
use common::error::Error;
use common::msg::Msg;
use common::ports::outbound::FileSystem;
use std::sync::Arc;

const AGENT_STATE_FILENAME: &str = "agent_state.json";

/// agent_state.json のスキーマ（メッセージと、任意の pending_input）
///
/// 既存バージョンでは Vec<Msg> のみを保存していたため、読み込み時はまず
/// AgentStateFile としてのパースを試み、失敗した場合は Vec<Msg> として扱う。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentStateFile {
    pub messages: Vec<Msg>,
    #[serde(default)]
    pub pending_input: Option<PendingInput>,
}

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

    /// pending_input を含めた agent_state.json を読み込む（存在しない場合は Ok(None)）。
    pub fn load_state_file(
        &self,
        session_dir: &SessionDir,
    ) -> Result<Option<AgentStateFile>, Error> {
        let path = Self::path(session_dir);
        if !self.fs.exists(&path) {
            return Ok(None);
        }
        let s = self.fs.read_to_string(&path)?;
        // まず新スキーマでパース
        match serde_json::from_str::<AgentStateFile>(&s) {
            Ok(f) => Ok(Some(f)),
            Err(_) => {
                // 互換性のため、古い Vec<Msg> 形式も受け入れる
                let msgs: Vec<Msg> =
                    serde_json::from_str(&s).map_err(|e| Error::json(e.to_string()))?;
                Ok(Some(AgentStateFile {
                    messages: msgs,
                    pending_input: None,
                }))
            }
        }
    }

    /// AgentStateFile 全体を書き戻すヘルパー。
    pub fn save_state_file(
        &self,
        session_dir: &SessionDir,
        state: &AgentStateFile,
    ) -> Result<(), Error> {
        let path = Self::path(session_dir);
        let json =
            serde_json::to_string(state).map_err(|e| Error::json(e.to_string()))?;
        self.fs.write(&path, &json)
    }

    /// pending_input のみを差し替えるヘルパー（messages は維持）。
    pub fn save_pending_input(
        &self,
        session_dir: &SessionDir,
        pending: Option<PendingInput>,
    ) -> Result<(), Error> {
        let mut state = self
            .load_state_file(session_dir)?
            .unwrap_or_else(|| AgentStateFile {
                messages: Vec::new(),
                pending_input: None,
            });
        state.pending_input = pending;
        self.save_state_file(session_dir, &state)
    }
}

impl AgentStateSaver for FileAgentStateStorage {
    fn save(&self, session_dir: &SessionDir, messages: &[Msg]) -> Result<(), Error> {
        let mut state = self
            .load_state_file(session_dir)?
            .unwrap_or_else(|| AgentStateFile {
                messages: Vec::new(),
                pending_input: None,
            });
        state.messages = messages.to_vec();
        self.save_state_file(session_dir, &state)
    }

    fn clear(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let path = Self::path(session_dir);
        if self.fs.exists(&path) {
            self.fs.remove_file(&path)?;
        }
        Ok(())
    }

    fn clear_resume_keep_pending(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let state = self
            .load_state_file(session_dir)?
            .unwrap_or_else(|| AgentStateFile {
                messages: Vec::new(),
                pending_input: None,
            });
        if state.pending_input.is_some() {
            let kept = AgentStateFile {
                messages: Vec::new(),
                pending_input: state.pending_input,
            };
            self.save_state_file(session_dir, &kept)
        } else {
            self.clear(session_dir)
        }
    }
}

impl AgentStateLoader for FileAgentStateStorage {
    fn load(&self, session_dir: &SessionDir) -> Result<Option<Vec<Msg>>, Error> {
        Ok(self
            .load_state_file(session_dir)?
            .map(|f| f.messages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;
    use common::domain::PolicyStatus;

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
        assert_eq!(loaded.clone().unwrap(), msgs);

        // 新スキーマ（AgentStateFile）としても整合性があること
        let state = storage.load_state_file(&session_dir).unwrap().unwrap();
        assert_eq!(state.messages, msgs);
        assert!(state.pending_input.is_none());
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

    #[test]
    fn test_save_and_load_pending_input() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(fs);

        let msgs = vec![Msg::user("hello")];
        storage.save(&session_dir, &msgs).unwrap();

        let pending = common::domain::PendingInput {
            text: "echo hi".to_string(),
            policy: PolicyStatus::Allowed,
            created_at_unix_ms: 123,
            source: "test".to_string(),
        };

        storage
            .save_pending_input(&session_dir, Some(pending.clone()))
            .unwrap();

        let state = storage.load_state_file(&session_dir).unwrap().unwrap();
        assert_eq!(state.messages, msgs);
        assert_eq!(state.pending_input, Some(pending));
    }
}
