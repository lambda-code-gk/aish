use crate::wiring::{wire_ai, App};
use std::env;
use std::fs;

fn app() -> App {
    wire_ai(false, false)
}

#[test]
fn test_session_from_env_no_env_var() {
    let original = env::var("AISH_SESSION").ok();
    env::remove_var("AISH_SESSION");

    let app = app();
    let session_dir = app.env_resolver.session_dir_from_env();
    assert!(session_dir.is_none());
    assert!(!app.ai_use_case.session_is_valid(&session_dir));

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

    let app = app();
    let session_dir_opt = app.env_resolver.session_dir_from_env();
    assert!(app.ai_use_case.session_is_valid(&session_dir_opt));
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

    let app = app();
    let session_dir_opt = app.env_resolver.session_dir_from_env();
    assert!(!app.ai_use_case.session_is_valid(&session_dir_opt));

    if let Some(val) = original {
        env::set_var("AISH_SESSION", val);
    } else {
        env::remove_var("AISH_SESSION");
    }
}
