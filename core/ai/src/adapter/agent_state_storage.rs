//! エージェント状態（続き用）を agent_state.json で保存・読み込みするアダプタ
//!
//! - agent_state.json: messages 専用（AI 側のみが read/write）
//! - pending_input.json: PendingInput 専用（AI が書き、AISH が読む/削除する）。競合回避のため完全分離。

use crate::ports::outbound::{AgentStateLoader, AgentStateSaver};
use common::domain::{PendingInput, SessionDir};
use common::error::Error;
use common::msg::Msg;
use common::ports::outbound::FileSystem;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const AGENT_STATE_FILENAME: &str = "agent_state.json";
const PENDING_INPUT_FILENAME: &str = "pending_input.json";

/// agent_state.json のスキーマ（messages のみ。pending は pending_input.json に分離）
///
/// 既存バージョンでは Vec<Msg> のみを保存していたため、読み込み時はまず
/// AgentStateFile としてのパースを試み、失敗した場合は Vec<Msg> として扱う。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentStateFile {
    pub messages: Vec<Msg>,
}

/// セッション dir に agent_state.json で Vec<Msg> を保存する実装
pub struct FileAgentStateStorage {
    fs: Arc<dyn FileSystem>,
}

impl FileAgentStateStorage {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    fn path_agent_state(session_dir: &SessionDir) -> std::path::PathBuf {
        session_dir.as_ref().join(AGENT_STATE_FILENAME)
    }

    fn path_pending_input(session_dir: &SessionDir) -> std::path::PathBuf {
        session_dir.as_ref().join(PENDING_INPUT_FILENAME)
    }

    /// 一時ファイルに書き込んでから rename で置換し、破損を防ぐ。
    fn write_atomic(&self, target: &Path, contents: &str) -> Result<(), Error> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        let base = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let tmp_name = format!(".{}.tmp.{}.{}", base, std::process::id(), millis);
        let tmp = parent.join(tmp_name);
        self.fs.write(&tmp, contents)?;
        self.fs.rename(&tmp, target)
    }

    /// agent_state.json を読み込む（存在しない場合は Ok(None)）。messages のみ。
    pub fn load_state_file(
        &self,
        session_dir: &SessionDir,
    ) -> Result<Option<AgentStateFile>, Error> {
        let path = Self::path_agent_state(session_dir);
        if !self.fs.exists(&path) {
            return Ok(None);
        }
        let s = self.fs.read_to_string(&path)?;
        match serde_json::from_str::<AgentStateFile>(&s) {
            Ok(f) => Ok(Some(f)),
            Err(_) => {
                let msgs: Vec<Msg> =
                    serde_json::from_str(&s).map_err(|e| Error::json(e.to_string()))?;
                Ok(Some(AgentStateFile { messages: msgs }))
            }
        }
    }

    /// agent_state.json に messages のみを atomic write する。
    pub fn save_state_file(
        &self,
        session_dir: &SessionDir,
        state: &AgentStateFile,
    ) -> Result<(), Error> {
        let path = Self::path_agent_state(session_dir);
        let json = serde_json::to_string(state).map_err(|e| Error::json(e.to_string()))?;
        self.write_atomic(&path, &json)
    }

    /// pending_input.json 専用の保存/削除。agent_state.json には一切触らない。
    /// - Some(p): pending_input.json に atomic write
    /// - None: pending_input.json があれば remove（無ければ無視）
    pub fn save_pending_input(
        &self,
        session_dir: &SessionDir,
        pending: Option<PendingInput>,
    ) -> Result<(), Error> {
        let path = Self::path_pending_input(session_dir);
        match pending {
            Some(p) => {
                let json =
                    serde_json::to_string(&p).map_err(|e| Error::json(e.to_string()))?;
                self.write_atomic(&path, &json)
            }
            None => {
                if self.fs.exists(&path) {
                    self.fs.remove_file(&path)?;
                }
                Ok(())
            }
        }
    }
}

impl AgentStateSaver for FileAgentStateStorage {
    fn save(&self, session_dir: &SessionDir, messages: &[Msg]) -> Result<(), Error> {
        let mut state = self
            .load_state_file(session_dir)?
            .unwrap_or_else(|| AgentStateFile {
                messages: Vec::new(),
            });
        state.messages = messages.to_vec();
        self.save_state_file(session_dir, &state)
    }

    fn clear(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let path = Self::path_agent_state(session_dir);
        if self.fs.exists(&path) {
            self.fs.remove_file(&path)?;
        }
        Ok(())
    }

    /// messages を消す（agent_state を空に）。pending_input.json は触らない。
    fn clear_resume_keep_pending(&self, session_dir: &SessionDir) -> Result<(), Error> {
        self.save_state_file(
            session_dir,
            &AgentStateFile {
                messages: Vec::new(),
            },
        )
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
        let storage = FileAgentStateStorage::new(Arc::clone(&fs));
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

        let state = storage.load_state_file(&session_dir).unwrap().unwrap();
        assert_eq!(state.messages, msgs);
    }

    #[test]
    fn test_agent_state_load_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(Arc::clone(&fs));
        let loaded = storage.load(&session_dir).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_agent_state_clear_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(Arc::clone(&fs));
        storage
            .save(&session_dir, &[Msg::user("x")])
            .unwrap();
        assert!(storage.load(&session_dir).unwrap().is_some());
        storage.clear(&session_dir).unwrap();
        assert!(storage.load(&session_dir).unwrap().is_none());
    }

    #[test]
    fn test_save_pending_input_creates_pending_input_json() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(Arc::clone(&fs));

        let pending = PendingInput {
            text: "echo hi".to_string(),
            policy: PolicyStatus::Allowed,
            created_at_unix_ms: 123,
            source: "test".to_string(),
        };

        storage
            .save_pending_input(&session_dir, Some(pending.clone()))
            .unwrap();

        let path = session_dir.as_ref().join(PENDING_INPUT_FILENAME);
        assert!(fs.exists(&path));
        let s = fs.read_to_string(&path).unwrap();
        let loaded: PendingInput = serde_json::from_str(&s).unwrap();
        assert_eq!(loaded.text, pending.text);
        assert_eq!(loaded.policy, pending.policy);
    }

    #[test]
    fn test_save_pending_input_none_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(Arc::clone(&fs));

        let pending = PendingInput {
            text: "git status".to_string(),
            policy: PolicyStatus::Allowed,
            created_at_unix_ms: 0,
            source: "test".to_string(),
        };
        storage
            .save_pending_input(&session_dir, Some(pending))
            .unwrap();
        let path = session_dir.as_ref().join(PENDING_INPUT_FILENAME);
        assert!(fs.exists(&path));

        storage.save_pending_input(&session_dir, None).unwrap();
        assert!(!fs.exists(&path));
    }

    #[test]
    fn test_save_pending_input_does_not_touch_agent_state() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(Arc::clone(&fs));

        let msgs = vec![Msg::user("hello")];
        storage.save(&session_dir, &msgs).unwrap();

        let pending = PendingInput {
            text: "echo hi".to_string(),
            policy: PolicyStatus::Allowed,
            created_at_unix_ms: 123,
            source: "test".to_string(),
        };
        storage
            .save_pending_input(&session_dir, Some(pending))
            .unwrap();

        let loaded = storage.load(&session_dir).unwrap().unwrap();
        assert_eq!(loaded, msgs);
    }

    #[test]
    fn test_clear_resume_keep_pending_keeps_pending_input_json() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = SessionDir::new(tmp.path().to_path_buf());
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let storage = FileAgentStateStorage::new(Arc::clone(&fs));

        storage.save(&session_dir, &[Msg::user("x")]).unwrap();
        let pending = PendingInput {
            text: "ls".to_string(),
            policy: PolicyStatus::Allowed,
            created_at_unix_ms: 0,
            source: "test".to_string(),
        };
        storage
            .save_pending_input(&session_dir, Some(pending))
            .unwrap();

        storage.clear_resume_keep_pending(&session_dir).unwrap();

        assert!(storage.load(&session_dir).unwrap().unwrap().is_empty());
        let path = session_dir.as_ref().join(PENDING_INPUT_FILENAME);
        assert!(fs.exists(&path));
    }
}
