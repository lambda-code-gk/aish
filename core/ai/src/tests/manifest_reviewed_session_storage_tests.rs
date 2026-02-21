//! ManifestReviewedSessionStorage のテスト

use crate::adapter::ManifestReviewedSessionStorage;
use crate::adapter::manifest_reviewed_session_storage::HistoryViewStrategy;
use crate::domain::History;
use crate::ports::outbound::SessionHistoryLoader;
use common::adapter::StdFileSystem;
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::FileSystem;
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn loader(load_max: usize) -> ManifestReviewedSessionStorage {
    ManifestReviewedSessionStorage::new(Arc::new(StdFileSystem), load_max)
}

#[test]
fn test_manifest_loader_with_tail_limit() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_manifest_loader_tail");
    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());
    let reviewed_dir = session_path.join("reviewed");
    fs::create_dir_all(&reviewed_dir).unwrap();
    fs::write(reviewed_dir.join("reviewed_001_user.txt"), "u1").unwrap();
    fs::write(reviewed_dir.join("reviewed_002_assistant.txt"), "a2").unwrap();
    fs::write(reviewed_dir.join("reviewed_003_user.txt"), "u3").unwrap();
    fs::write(
        session_path.join("manifest.jsonl"),
        "\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t1\",\"id\":\"001\",\"role\":\"user\",\"part_path\":\"part_001_user.txt\",\"reviewed_path\":\"reviewed/reviewed_001_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"aa\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t2\",\"id\":\"002\",\"role\":\"assistant\",\"part_path\":\"part_002_assistant.txt\",\"reviewed_path\":\"reviewed/reviewed_002_assistant.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"bb\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t3\",\"id\":\"003\",\"role\":\"user\",\"part_path\":\"part_003_user.txt\",\"reviewed_path\":\"reviewed/reviewed_003_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"cc\"}\n",
    )
    .unwrap();

    let history = loader(2).load(&session_dir).unwrap();
    assert_eq!(history.messages().len(), 2);
    assert_eq!(history.messages()[0].role, "assistant");
    assert_eq!(history.messages()[0].content, "a2");
    assert_eq!(history.messages()[1].role, "user");
    assert_eq!(history.messages()[1].content, "u3");

    fs::remove_dir_all(session_path).unwrap();
}

#[test]
fn test_manifest_loader_fallback_reviewed_tail_limit() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_manifest_loader_fallback");
    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());
    let reviewed_dir = session_path.join("reviewed");
    fs::create_dir_all(&reviewed_dir).unwrap();
    fs::write(reviewed_dir.join("reviewed_001_user.txt"), "u1").unwrap();
    fs::write(reviewed_dir.join("reviewed_002_assistant.txt"), "a2").unwrap();
    fs::write(reviewed_dir.join("reviewed_003_user.txt"), "u3").unwrap();

    let history = loader(2).load(&session_dir).unwrap();
    assert_eq!(history.messages().len(), 2);
    assert_eq!(history.messages()[0].role, "assistant");
    assert_eq!(history.messages()[0].content, "a2");
    assert_eq!(history.messages()[1].role, "user");
    assert_eq!(history.messages()[1].content, "u3");

    fs::remove_dir_all(session_path).unwrap();
}

#[test]
fn test_manifest_loader_inserts_compaction_summary_before_tail() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_manifest_loader_compaction");
    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());
    let reviewed_dir = session_path.join("reviewed");
    fs::create_dir_all(&reviewed_dir).unwrap();
    fs::write(reviewed_dir.join("reviewed_001_user.txt"), "u1").unwrap();
    fs::write(reviewed_dir.join("reviewed_002_assistant.txt"), "a2").unwrap();
    fs::write(reviewed_dir.join("reviewed_003_user.txt"), "u3").unwrap();
    fs::write(session_path.join("compaction_001_001.txt"), "summary old").unwrap();
    fs::write(
        session_path.join("manifest.jsonl"),
        "\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t1\",\"id\":\"001\",\"role\":\"user\",\"part_path\":\"part_001_user.txt\",\"reviewed_path\":\"reviewed/reviewed_001_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"aa\"}\n\
{\"kind\":\"compaction\",\"v\":1,\"ts\":\"tc\",\"from_id\":\"001\",\"to_id\":\"001\",\"summary_path\":\"compaction_001_001.txt\",\"method\":\"deterministic\",\"source_count\":1}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t2\",\"id\":\"002\",\"role\":\"assistant\",\"part_path\":\"part_002_assistant.txt\",\"reviewed_path\":\"reviewed/reviewed_002_assistant.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"bb\"}\n\
{\"kind\":\"message\",\"v\":1,\"ts\":\"t3\",\"id\":\"003\",\"role\":\"user\",\"part_path\":\"part_003_user.txt\",\"reviewed_path\":\"reviewed/reviewed_003_user.txt\",\"decision\":\"allow\",\"bytes\":2,\"hash64\":\"cc\"}\n",
    )
    .unwrap();

    let history = loader(2).load(&session_dir).unwrap();
    assert_eq!(history.messages().len(), 3);
    assert_eq!(history.messages()[0].role, "assistant");
    assert_eq!(history.messages()[0].content, "summary old");
    assert_eq!(history.messages()[1].content, "a2");
    assert_eq!(history.messages()[2].content, "u3");

    fs::remove_dir_all(session_path).unwrap();
}

struct StubStrategy(&'static str);

impl HistoryViewStrategy for StubStrategy {
    fn build_history(
        &self,
        _fs: &dyn FileSystem,
        _dir: &Path,
        _load_max: usize,
    ) -> Result<History, Error> {
        let mut history = History::new();
        history.push_assistant(self.0);
        Ok(history)
    }
}

#[test]
fn test_manifest_loader_uses_injected_strategy() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_manifest_loader_strategy");
    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());
    fs::write(session_path.join("manifest.jsonl"), "{}\n").unwrap();

    let loader = ManifestReviewedSessionStorage::with_strategies(
        Arc::new(StdFileSystem),
        10,
        Arc::new(StubStrategy("manifest-strategy")),
        Arc::new(StubStrategy("fallback-strategy")),
        None,
    );
    let history = loader.load(&session_dir).unwrap();
    assert_eq!(history.messages().len(), 1);
    assert_eq!(history.messages()[0].content, "manifest-strategy");

    fs::remove_dir_all(session_path).unwrap();
}

