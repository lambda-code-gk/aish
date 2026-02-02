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
fn test_save_response() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_save_response");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    let uc = use_case();
    let response = "This is a test response from the assistant.";
    let result = uc.save_response(&session_dir, response);
    assert!(result.is_ok());

    let entries: Vec<_> = fs::read_dir(&session_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|name| {
            let name_str = name.to_str().unwrap();
            name_str.starts_with("part_") && name_str.ends_with("_assistant.txt")
        })
        .collect();

    assert_eq!(entries.len(), 1);
    let file_path = session_dir.join(&entries[0]);
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, response);

    fs::remove_dir_all(&session_path).unwrap();
}

#[test]
fn test_save_response_with_user_part() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_save_response_with_user");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    fs::create_dir_all(&session_path).unwrap();
    let session_dir = SessionDir::new(session_path.clone());

    let user_part_id = "AZwJxha3";
    let user_part_file = session_path.join(format!("part_{}_user.txt", user_part_id));
    fs::write(&user_part_file, "User message").unwrap();

    let uc = use_case();
    let response = "This is a test response from the assistant.";
    let result = uc.save_response(&session_dir, response);
    assert!(result.is_ok());

    let entries: Vec<_> = fs::read_dir(&session_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|name| {
            let name_str = name.to_str().unwrap();
            name_str.starts_with("part_") && name_str.ends_with("_assistant.txt")
        })
        .collect();

    assert_eq!(entries.len(), 1);
    let file_path = session_dir.join(&entries[0]);
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, response);

    fs::remove_dir_all(&session_path).unwrap();
}

#[test]
fn test_save_response_nonexistent_session() {
    let temp_dir = std::env::temp_dir();
    let session_path = temp_dir.join("aish_test_nonexistent_save");

    if session_path.exists() {
        fs::remove_dir_all(&session_path).unwrap();
    }
    let session_dir = SessionDir::new(session_path);

    let uc = use_case();
    let response = "This is a test response.";
    let result = uc.save_response(&session_dir, response);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not valid"));
}
