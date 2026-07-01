//! `AI_EDITOR` / `VISUAL` / `EDITOR` による外部エディタ起動。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::adapters::outbound::prompt_comment_filter::filter_editor_draft;
use crate::domain::{is_substantive_prompt, PromptAcquisitionResult};

pub const PROMPT_TEMPLATE: &str = "\
<!-- ai-prompt: Enter your AISH prompt below. This block is ignored on send. -->

";

/// 環境変数の優先順位で editor コマンドを解決する。
pub fn resolve_editor_command_from_env() -> Option<Vec<String>> {
    for key in ["AI_EDITOR", "VISUAL", "EDITOR"] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        if let Ok(parts) = shell_words::split(&value) {
            if !parts.is_empty() {
                return Some(parts);
            }
        }
    }
    None
}

/// 一時 `.md` ファイルを作成しテンプレートを書き込む。
pub fn create_prompt_temp_file(
    dir: Option<&Path>,
) -> std::io::Result<(tempfile::NamedTempFile, PathBuf)> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("aish-prompt-").suffix(".md");
    let mut file = if let Some(dir) = dir {
        builder.tempfile_in(dir)?
    } else {
        builder.tempfile()?
    };
    file.write_all(PROMPT_TEMPLATE.as_bytes())?;
    file.flush()?;
    let path = file.path().to_path_buf();
    Ok((file, path))
}

/// 外部エディタで一時ファイルを開き、編集結果を取得する。
pub fn acquire_prompt_via_external_editor(
    command_parts: &[String],
    temp_path: &Path,
) -> PromptAcquisitionResult {
    if command_parts.is_empty() {
        return PromptAcquisitionResult::EditorFailed { exit_code: Some(1) };
    }

    let program = &command_parts[0];
    let mut args: Vec<&str> = command_parts[1..].iter().map(String::as_str).collect();
    args.push(temp_path.to_str().unwrap_or_default());

    let status = match Command::new(program).args(&args).status() {
        Ok(status) => status,
        Err(_) => return PromptAcquisitionResult::EditorFailed { exit_code: Some(1) },
    };

    if !status.success() {
        return PromptAcquisitionResult::EditorFailed {
            exit_code: status.code(),
        };
    }

    let raw = match std::fs::read_to_string(temp_path) {
        Ok(content) => content,
        Err(_) => return PromptAcquisitionResult::EditorFailed { exit_code: Some(1) },
    };

    let filtered = filter_editor_draft(&raw);
    if !is_substantive_prompt(&filtered) {
        return PromptAcquisitionResult::Empty;
    }

    PromptAcquisitionResult::Submitted { content: filtered }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn create_prompt_temp_file_honors_custom_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (_file, path) = create_prompt_temp_file(Some(dir.path())).expect("temp file");
        assert!(
            path.starts_with(dir.path()),
            "expected temp file under {:?}, got {}",
            dir.path(),
            path.display()
        );
    }

    #[test]
    fn unit_editor_precedence_prefers_ai_editor_then_visual_then_editor() {
        let _lock = env_lock();
        std::env::set_var("AI_EDITOR", "ai-editor-cmd");
        std::env::set_var("VISUAL", "visual-cmd");
        std::env::set_var("EDITOR", "editor-cmd");
        assert_eq!(
            resolve_editor_command_from_env().expect("cmd"),
            vec!["ai-editor-cmd".to_string()]
        );
        std::env::remove_var("AI_EDITOR");
        assert_eq!(
            resolve_editor_command_from_env().expect("cmd"),
            vec!["visual-cmd".to_string()]
        );
        std::env::remove_var("VISUAL");
        assert_eq!(
            resolve_editor_command_from_env().expect("cmd"),
            vec!["editor-cmd".to_string()]
        );
        std::env::remove_var("EDITOR");
        assert!(resolve_editor_command_from_env().is_none());
    }

    #[test]
    fn shell_words_splits_editor_with_args() {
        let _lock = env_lock();
        std::env::set_var("AI_EDITOR", "code --wait");
        assert_eq!(
            resolve_editor_command_from_env().expect("cmd"),
            vec!["code".to_string(), "--wait".to_string()]
        );
        std::env::remove_var("AI_EDITOR");
    }

    #[test]
    fn unit_empty_prompt_after_comment_strip_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("draft.md");
        fs::write(&path, PROMPT_TEMPLATE).expect("write");
        let script = "#!/bin/sh\n# leave template only\nexit 0\n".to_string();
        let editor = dir.path().join("noop.sh");
        fs::write(&editor, script).expect("write editor");
        let mut perms = fs::metadata(&editor).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&editor, perms).expect("chmod");

        let result =
            acquire_prompt_via_external_editor(&[editor.to_string_lossy().into_owned()], &path);
        assert_eq!(result, PromptAcquisitionResult::Empty);
    }

    #[test]
    fn unit_abnormal_editor_exit_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("draft.md");
        fs::write(&path, PROMPT_TEMPLATE).expect("write");
        let editor = dir.path().join("fail.sh");
        fs::write(&editor, "#!/bin/sh\nexit 1\n").expect("write");
        let mut perms = fs::metadata(&editor).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&editor, perms).expect("chmod");

        let result =
            acquire_prompt_via_external_editor(&[editor.to_string_lossy().into_owned()], &path);
        assert!(matches!(
            result,
            PromptAcquisitionResult::EditorFailed { exit_code: Some(1) }
        ));
    }

    #[test]
    fn external_editor_writes_multiline_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("draft.md");
        fs::write(&path, PROMPT_TEMPLATE).expect("write");
        let editor = dir.path().join("write.sh");
        let script = "#!/bin/sh\ncat > \"$1\" <<'EOF'\nline 1\nline 2\nline 3\nEOF\n".to_string();
        fs::write(&editor, script).expect("write");
        let mut perms = fs::metadata(&editor).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&editor, perms).expect("chmod");

        let result =
            acquire_prompt_via_external_editor(&[editor.to_string_lossy().into_owned()], &path);
        assert_eq!(
            result,
            PromptAcquisitionResult::Submitted {
                content: "line 1\nline 2\nline 3".to_string()
            }
        );
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().expect("lock")
    }
}
