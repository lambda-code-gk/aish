//! SessionHistoryLoader（Part ファイル履歴読み込み）のテスト。adapter と port にのみ依存する。

use crate::adapter::PartSessionStorage;
use crate::ports::outbound::SessionHistoryLoader;
use common::adapter::{StdClock, StdFileSystem};
use common::domain::SessionDir;
use common::part_id::StdIdGenerator;
use std::fs;
use std::sync::Arc;

fn history_loader() -> PartSessionStorage {
    PartSessionStorage::new(
        Arc::new(StdFileSystem),
        Arc::new(StdIdGenerator::new(Arc::new(StdClock))),
    )
}

#[test]
fn test_load_history_no_directory() {
    let temp_dir = std::env::temp_dir();
    let non_existent_dir = temp_dir.join("aish_test_nonexistent_session");
    let session_dir = SessionDir::new(non_existent_dir);

    let loader = history_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();
    assert!(history.is_empty());
}

#[test]
fn test_load_history_empty_session_dir() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_empty_session");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    let loader = history_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();
    assert!(history.is_empty());

    fs::remove_dir_all(&session_path).unwrap();
}

#[test]
fn test_load_history_with_files() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_session_with_files");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    let part1 = session_path.join("part_20240101_120000_user.txt");
    let part2 = session_path.join("part_20240102_120000_assistant.txt");
    let part3 = session_path.join("part_20240103_120000_user.txt");

    fs::write(&part1, "First part content").unwrap();
    fs::write(&part2, "Second part content").unwrap();
    fs::write(&part3, "Third part content").unwrap();

    let loader = history_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();

    assert_eq!(history.messages().len(), 3);
    assert_eq!(history.messages()[0].role, "user");
    assert_eq!(history.messages()[0].content, "First part content");
    assert_eq!(history.messages()[1].role, "assistant");
    assert_eq!(history.messages()[1].content, "Second part content");
    assert_eq!(history.messages()[2].role, "user");
    assert_eq!(history.messages()[2].content, "Third part content");

    fs::remove_dir_all(&session_path).unwrap();
}

#[test]
fn test_load_history_ignores_non_part_files() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_session_ignore_files");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    let part_file = session_path.join("part_20240101_120000_user.txt");
    let other_file = session_path.join("other_file.txt");

    fs::write(&part_file, "Part content").unwrap();
    fs::write(&other_file, "Other content").unwrap();

    let loader = history_loader();
    let result = loader.load(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();

    assert_eq!(history.messages().len(), 1);
    assert_eq!(history.messages()[0].role, "user");
    assert_eq!(history.messages()[0].content, "Part content");

    fs::remove_dir_all(&session_path).unwrap();
}
