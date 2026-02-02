use crate::usecase::app::AiUseCase;
use common::domain::SessionDir;
use std::fs;
use std::sync::Arc;

fn use_case() -> AiUseCase {
    AiUseCase::new(
        Arc::new(common::adapter::StdFileSystem),
        Arc::new(common::part_id::StdIdGenerator::new(Arc::new(
            common::adapter::StdClock,
        ))),
        Arc::new(common::adapter::StdEnvResolver),
        Arc::new(common::adapter::StdProcess),
    )
}

#[test]
fn test_load_history_no_directory() {
    let temp_dir = std::env::temp_dir();
    let non_existent_dir = temp_dir.join("aish_test_nonexistent_session");
    let session_dir = SessionDir::new(non_existent_dir);

    let uc = use_case();
    let result = uc.load_history(&session_dir);
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

    let uc = use_case();
    let result = uc.load_history(&session_dir);
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

    let uc = use_case();
    let result = uc.load_history(&session_dir);
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

    let uc = use_case();
    let result = uc.load_history(&session_dir);
    assert!(result.is_ok());
    let history = result.unwrap();

    assert_eq!(history.messages().len(), 1);
    assert_eq!(history.messages()[0].role, "user");
    assert_eq!(history.messages()[0].content, "Part content");

    fs::remove_dir_all(&session_path).unwrap();
}
