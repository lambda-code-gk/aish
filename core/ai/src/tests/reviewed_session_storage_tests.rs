//! ReviewedSessionStorage（reviewed_* ファイルのみ読み込み）のテスト。

use crate::adapter::reviewed_session_storage::ReviewedSessionStorage;
use crate::ports::outbound::SessionHistoryLoader;
use common::adapter::StdFileSystem;
use common::domain::SessionDir;
use std::fs;
use std::sync::Arc;

fn reviewed_loader() -> ReviewedSessionStorage {
    ReviewedSessionStorage::new(Arc::new(StdFileSystem))
}

#[test]
fn test_load_reviewed_only_empty_dir() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_reviewed_empty");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    let loader = reviewed_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();
    assert!(history.is_empty());

    fs::remove_dir_all(&session_path).unwrap();
}

#[test]
fn test_load_reviewed_from_reviewed_files() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_reviewed_with_files");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    // ID は辞書順＝時系列のため、001 < 002 < 003 の順で並ぶ
    let r1 = session_path.join("reviewed_ABC12001_user.txt");
    let r2 = session_path.join("reviewed_ABC12002_assistant.txt");
    let r3 = session_path.join("reviewed_ABC12003_user.txt");
    fs::write(&r1, "First user").unwrap();
    fs::write(&r2, "Assistant reply").unwrap();
    fs::write(&r3, "Second user").unwrap();

    let loader = reviewed_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();

    assert_eq!(history.messages().len(), 3);
    assert_eq!(history.messages()[0].role, "user");
    assert_eq!(history.messages()[0].content, "First user");
    assert_eq!(history.messages()[1].role, "assistant");
    assert_eq!(history.messages()[1].content, "Assistant reply");
    assert_eq!(history.messages()[2].role, "user");
    assert_eq!(history.messages()[2].content, "Second user");

    fs::remove_dir_all(&session_path).unwrap();
}

#[test]
fn test_load_reviewed_ignores_part_files() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_reviewed_ignores_part");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    let reviewed_file = session_path.join("reviewed_XYZ99999_user.txt");
    let part_file = session_path.join("part_ABC12xyz_user.txt");
    fs::write(&reviewed_file, "Reviewed content").unwrap();
    fs::write(&part_file, "Part content (should be ignored)").unwrap();

    let loader = reviewed_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();

    assert_eq!(history.messages().len(), 1);
    assert_eq!(history.messages()[0].role, "user");
    assert_eq!(history.messages()[0].content, "Reviewed content");

    fs::remove_dir_all(&session_path).unwrap();
}

#[test]
fn test_load_reviewed_no_directory() {
    let temp_dir = std::env::temp_dir();
    let non_existent = temp_dir.join("aish_test_reviewed_nonexistent");
    let session_dir = SessionDir::new(non_existent);

    let loader = reviewed_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();
    assert!(history.is_empty());
}
