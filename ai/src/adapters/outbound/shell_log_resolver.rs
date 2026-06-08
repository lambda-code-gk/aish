//! `ai ask` のシェルログパス解決（0019）。filesystem I/O は adapter 側。

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::domain::{
    validate_session_id, ShellLogChoice, ShellLogResolveError, AI_ASK_LOG_SESSION,
};

/// 優先: `--no-log` → `--log` → `--session` → `AI_ASK_LOG=session`。
pub fn resolve_shell_log_for_ask(
    no_log: bool,
    log_cli: Option<&Path>,
    session_cli: Option<&str>,
    ai_ask_log: Option<&str>,
    aish_session_dir: Option<&Path>,
) -> Result<ShellLogChoice, ShellLogResolveError> {
    if no_log {
        return Ok(ShellLogChoice::None);
    }
    if let Some(path) = log_cli {
        return Ok(ShellLogChoice::Path(path.to_path_buf()));
    }
    if let Some(id) = session_cli {
        let path = resolve_session_current_log(id, aish_session_dir)?;
        return Ok(ShellLogChoice::Path(path));
    }
    match ai_ask_log {
        None => Ok(ShellLogChoice::None),
        Some(AI_ASK_LOG_SESSION) => {
            let dir = aish_session_dir.ok_or(ShellLogResolveError::SessionDirRequired)?;
            let path = open_session_current_log(dir, "current_log")?;
            Ok(ShellLogChoice::Path(path))
        }
        Some(other) => Err(ShellLogResolveError::InvalidAiAskLog(other.to_string())),
    }
}

fn resolve_session_current_log(
    id: &str,
    session_dir: Option<&Path>,
) -> Result<PathBuf, ShellLogResolveError> {
    validate_session_id(id)?;
    let dir = session_dir.ok_or(ShellLogResolveError::SessionDirRequiredForFlag)?;
    let dir = dir
        .canonicalize()
        .map_err(|e| ShellLogResolveError::Unreadable(dir.display().to_string(), e.to_string()))?;
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return Err(ShellLogResolveError::SessionIdMismatch {
            id: id.to_string(),
            dir: dir.display().to_string(),
        });
    };
    if name != id {
        return Err(ShellLogResolveError::SessionIdMismatch {
            id: id.to_string(),
            dir: dir.display().to_string(),
        });
    }
    let log = open_session_current_log(&dir, "current_log")?;
    Ok(log)
}

fn open_session_current_log(
    session_dir: &Path,
    link_name: &str,
) -> Result<PathBuf, ShellLogResolveError> {
    let session_dir = session_dir.canonicalize().map_err(|e| {
        ShellLogResolveError::Unreadable(session_dir.display().to_string(), e.to_string())
    })?;
    let current_log = session_dir.join(link_name);

    let meta = std::fs::metadata(&current_log).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ShellLogResolveError::NotFound(current_log.display().to_string())
        } else {
            ShellLogResolveError::Unreadable(current_log.display().to_string(), e.to_string())
        }
    })?;
    if meta.is_dir() {
        return Err(ShellLogResolveError::Unreadable(
            current_log.display().to_string(),
            "is a directory".into(),
        ));
    }

    let resolved = current_log.canonicalize().map_err(|e| {
        ShellLogResolveError::Unreadable(current_log.display().to_string(), e.to_string())
    })?;
    if !resolved.starts_with(&session_dir) {
        return Err(ShellLogResolveError::Unreadable(
            current_log.display().to_string(),
            "current_log resolves outside AISH_SESSION_DIR".into(),
        ));
    }

    File::open(&resolved).map_err(|e| {
        ShellLogResolveError::Unreadable(resolved.display().to_string(), e.to_string())
    })?;

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    fn session_fixture() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let session = root.path().join("002f15d02b54");
        fs::create_dir(&session).expect("mkdir");
        fs::write(session.join("log.jsonl"), "x").expect("write");
        symlink("log.jsonl", session.join("current_log")).expect("link");
        (root, session)
    }

    #[test]
    fn no_log_wins_over_env() {
        let (_root, session) = session_fixture();
        let got = resolve_shell_log_for_ask(true, None, None, Some("session"), Some(&session))
            .expect("ok");
        assert_eq!(got, ShellLogChoice::None);
    }

    #[test]
    fn log_cli_wins_over_session() {
        let (_root, session) = session_fixture();
        let got = resolve_shell_log_for_ask(
            false,
            Some(Path::new("/tmp/a.jsonl")),
            Some("002f15d02b54"),
            None,
            Some(&session),
        )
        .expect("ok");
        assert_eq!(got, ShellLogChoice::Path(PathBuf::from("/tmp/a.jsonl")));
    }

    #[test]
    fn invalid_ai_ask_log_errors() {
        let err = resolve_shell_log_for_ask(false, None, None, Some("yes"), None).unwrap_err();
        assert!(matches!(err, ShellLogResolveError::InvalidAiAskLog(_)));
    }

    #[test]
    fn session_rejects_traversal() {
        let (_root, session) = session_fixture();
        let err =
            resolve_shell_log_for_ask(false, None, Some("../x"), None, Some(&session)).unwrap_err();
        assert!(matches!(err, ShellLogResolveError::InvalidSessionId(_)));
    }

    #[test]
    fn session_rejects_non_hex_and_wrong_length() {
        let (_root, session) = session_fixture();
        for bad in ["123", "002F15D02B54", "002f15d02b54extra", "gggggggggggg"] {
            let err = resolve_shell_log_for_ask(false, None, Some(bad), None, Some(&session))
                .unwrap_err();
            assert!(
                matches!(err, ShellLogResolveError::InvalidSessionId(_)),
                "expected invalid for {bad:?}"
            );
        }
    }

    #[test]
    fn session_requires_aish_session_dir() {
        let err =
            resolve_shell_log_for_ask(false, None, Some("002f15d02b54"), None, None).unwrap_err();
        assert!(matches!(
            err,
            ShellLogResolveError::SessionDirRequiredForFlag
        ));
    }

    #[test]
    fn session_id_must_match_aish_session_dir() {
        let (_root, session) = session_fixture();
        let err =
            resolve_shell_log_for_ask(false, None, Some("000000000001"), None, Some(&session))
                .unwrap_err();
        assert!(matches!(
            err,
            ShellLogResolveError::SessionIdMismatch { .. }
        ));
    }

    #[test]
    fn session_via_aish_session_dir() {
        let (_root, session) = session_fixture();
        let got =
            resolve_shell_log_for_ask(false, None, Some("002f15d02b54"), None, Some(&session))
                .expect("ok");
        assert_eq!(got, ShellLogChoice::Path(session.join("log.jsonl")));
    }

    #[test]
    fn ai_ask_log_session_reads_current_log() {
        let (_root, session) = session_fixture();
        let got = resolve_shell_log_for_ask(false, None, None, Some("session"), Some(&session))
            .expect("ok");
        assert_eq!(got, ShellLogChoice::Path(session.join("log.jsonl")));
    }

    #[test]
    fn rejects_dangling_current_log_symlink() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = root.path().join("002f15d02b54");
        fs::create_dir(&session).expect("mkdir");
        symlink("missing.jsonl", session.join("current_log")).expect("link");

        let err = resolve_shell_log_for_ask(false, None, None, Some("session"), Some(&session))
            .unwrap_err();
        assert!(matches!(err, ShellLogResolveError::NotFound(_)));
    }

    #[test]
    fn rejects_current_log_symlink_outside_session_dir() {
        let root = tempfile::tempdir().expect("tempdir");
        let outside = root.path().join("outside.jsonl");
        fs::write(&outside, "secret").expect("write");
        let session = root.path().join("002f15d02b54");
        fs::create_dir(&session).expect("mkdir");
        symlink(
            outside.canonicalize().expect("canon"),
            session.join("current_log"),
        )
        .expect("link");

        let err = resolve_shell_log_for_ask(false, None, None, Some("session"), Some(&session))
            .unwrap_err();
        assert!(matches!(err, ShellLogResolveError::Unreadable(_, _)));
    }
}
