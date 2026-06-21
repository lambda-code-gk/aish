//! bare `ai` 用の対話的プロンプト入力 façade（composition root から呼ぶ）。

use std::io::IsTerminal;

use crate::adapters::inbound::reedline_prompt::acquire_prompt_via_reedline;
use crate::domain::{
    should_enter_interactive_prompt_mode, AskInvocationSource, PromptAcquisitionResult,
};

use super::{
    acquire_prompt_via_external_editor, create_prompt_temp_file, resolve_editor_command_from_env,
};

pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

pub fn acquire_interactive_prompt(
    invocation: AskInvocationSource,
) -> std::io::Result<Option<PromptAcquisitionResult>> {
    if !should_enter_interactive_prompt_mode(invocation, stdin_is_tty()) {
        return Ok(None);
    }

    if let Some(command) = resolve_editor_command_from_env() {
        let (_file, path) = create_prompt_temp_file(None)?;
        let result = acquire_prompt_via_external_editor(&command, &path);
        return Ok(Some(result));
    }

    Ok(Some(acquire_prompt_via_reedline()?))
}

#[cfg(test)]
pub fn acquire_interactive_prompt_with_editor_in_dir(
    invocation: AskInvocationSource,
    editor: &[String],
    temp_dir: &std::path::Path,
) -> PromptAcquisitionResult {
    assert!(should_enter_interactive_prompt_mode(invocation, true));
    let (_file, path) = create_prompt_temp_file(Some(temp_dir)).expect("temp file");
    acquire_prompt_via_external_editor(editor, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn fake_editor_prompt_is_submitted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let editor = dir.path().join("fake.sh");
        fs::write(&editor, "#!/bin/sh\necho \"hello from editor\" > \"$1\"\n").expect("write");
        let mut perms = fs::metadata(&editor).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&editor, perms).expect("chmod");

        let result = acquire_interactive_prompt_with_editor_in_dir(
            AskInvocationSource::BareRoot,
            &[editor.to_string_lossy().into_owned()],
            dir.path(),
        );
        assert_eq!(
            result,
            PromptAcquisitionResult::Submitted {
                content: "hello from editor".to_string()
            }
        );
    }
}
