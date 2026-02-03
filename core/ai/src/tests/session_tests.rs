use crate::usecase::app::AiUseCase;
use crate::wiring::wire_ai;
use std::env;
use std::fs;
use std::sync::Arc;

fn use_case() -> Arc<AiUseCase> {
    wire_ai().ai_use_case
}

#[test]
fn test_session_from_env_no_env_var() {
    let original = env::var("AISH_SESSION").ok();
    env::remove_var("AISH_SESSION");

    let uc = use_case();
    let session_dir = uc.env_resolver.session_dir_from_env();
    assert!(session_dir.is_none());
    assert!(!uc.session_is_valid(&session_dir));

    if let Some(val) = original {
        env::set_var("AISH_SESSION", val);
    }
}

#[test]
fn test_session_from_env_with_existing_dir() {
    let temp_dir = std::env::temp_dir();
    let session_dir = temp_dir.join("aish_test_session_valid");

    if session_dir.exists() {
        fs::remove_dir_all(&session_dir).unwrap();
    }
    fs::create_dir_all(&session_dir).unwrap();

    let original = env::var("AISH_SESSION").ok();
    env::set_var("AISH_SESSION", session_dir.to_str().unwrap());

    let uc = use_case();
    let session_dir_opt = uc.env_resolver.session_dir_from_env();
    assert!(uc.session_is_valid(&session_dir_opt));
    assert_eq!(
        session_dir_opt.as_ref().unwrap().as_path(),
        session_dir.as_path()
    );

    if let Some(val) = original {
        env::set_var("AISH_SESSION", val);
    } else {
        env::remove_var("AISH_SESSION");
    }
    fs::remove_dir_all(&session_dir).unwrap();
}

#[test]
fn test_session_from_env_with_nonexistent_dir() {
    let temp_dir = std::env::temp_dir();
    let session_dir = temp_dir.join("aish_test_session_nonexistent");

    if session_dir.exists() {
        fs::remove_dir_all(&session_dir).unwrap();
    }

    let original = env::var("AISH_SESSION").ok();
    env::set_var("AISH_SESSION", session_dir.to_str().unwrap());

    let uc = use_case();
    let session_dir_opt = uc.env_resolver.session_dir_from_env();
    assert!(!uc.session_is_valid(&session_dir_opt));

    if let Some(val) = original {
        env::set_var("AISH_SESSION", val);
    } else {
        env::remove_var("AISH_SESSION");
    }
}
