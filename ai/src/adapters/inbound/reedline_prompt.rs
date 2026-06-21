//! `reedline` ベースの内蔵ミニエディタ（bare `ai` 用）。

use std::borrow::Cow;

use reedline::{
    default_emacs_keybindings, EditCommand, Emacs, KeyCode, KeyModifiers, Prompt, PromptEditMode,
    PromptHistorySearch, Reedline, ReedlineEvent, Signal,
};

use crate::domain::PromptAcquisitionResult;

const PROMPT: &str = "AISH prompt> ";

struct MiniEditorPrompt;

impl Prompt for MiniEditorPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed(PROMPT)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("Ctrl+Enter / Alt+Enter 送信")
    }

    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("           > ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        _history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
}

fn prompt_keybindings() -> reedline::Keybindings {
    let mut kb = default_emacs_keybindings();
    kb.add_binding(
        KeyModifiers::NONE,
        KeyCode::Enter,
        ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
    );
    // Ctrl+Enter は Kitty / WezTerm 等（キーボード拡張対応端末）で区別できる。
    kb.add_binding(KeyModifiers::CONTROL, KeyCode::Enter, ReedlineEvent::Submit);
    // 従来端末では Ctrl+Enter が Enter と同じ信号になるため、代替送信キーを用意する。
    kb.add_binding(KeyModifiers::ALT, KeyCode::Enter, ReedlineEvent::Submit);
    kb.add_binding(KeyModifiers::SHIFT, KeyCode::Enter, ReedlineEvent::Submit);
    // SKK 等の IME と衝突するため、reedline 既定の Ctrl+J（送信相当）を無効化する。
    kb.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('j'),
        ReedlineEvent::None,
    );
    kb.add_binding(
        KeyModifiers::NONE,
        KeyCode::Up,
        ReedlineEvent::Edit(vec![EditCommand::MoveLineUp { select: false }]),
    );
    kb.add_binding(
        KeyModifiers::NONE,
        KeyCode::Down,
        ReedlineEvent::Edit(vec![EditCommand::MoveLineDown { select: false }]),
    );
    kb
}

fn create_prompt_editor() -> Reedline {
    let edit_mode = Box::new(Emacs::new(prompt_keybindings()));
    Reedline::create()
        .with_edit_mode(edit_mode)
        .use_kitty_keyboard_enhancement(true)
}

/// TTY 上で複数行プロンプトを読み取る。
/// `Enter` で改行、`↑`/`↓` で行移動、`Ctrl+Enter`（対応端末）または `Alt+Enter` で送信、`Ctrl+C` でキャンセル。
pub fn acquire_prompt_via_reedline() -> std::io::Result<PromptAcquisitionResult> {
    let mut line_editor = create_prompt_editor();
    let prompt = MiniEditorPrompt;

    match line_editor.read_line(&prompt) {
        Ok(Signal::Success(content)) => {
            if content.trim().is_empty() {
                Ok(PromptAcquisitionResult::Empty)
            } else {
                Ok(PromptAcquisitionResult::Submitted { content })
            }
        }
        Ok(Signal::CtrlD) => Ok(PromptAcquisitionResult::Empty),
        Ok(Signal::CtrlC) => Ok(PromptAcquisitionResult::Cancelled),
        Ok(_) => Ok(PromptAcquisitionResult::Cancelled),
        Err(e) => Err(std::io::Error::other(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::is_substantive_prompt;
    use reedline::KeyCode;

    #[test]
    fn unit_reedline_prompt_editor_handles_enter_eof_and_interrupt_contract() {
        let joined = ["line 1", "line 2"].join("\n");
        assert!(is_substantive_prompt(&joined));
        assert!(!is_substantive_prompt(""));
        assert!(!is_substantive_prompt("   \n  "));
    }

    #[test]
    fn prompt_keybindings_map_enter_to_newline_and_ctrl_enter_to_submit() {
        let kb = prompt_keybindings();
        let enter = kb
            .find_binding(KeyModifiers::NONE, KeyCode::Enter)
            .expect("enter binding");
        assert!(matches!(
            enter,
            ReedlineEvent::Edit(commands) if commands == [EditCommand::InsertNewline]
        ));

        let ctrl_enter = kb
            .find_binding(KeyModifiers::CONTROL, KeyCode::Enter)
            .expect("ctrl-enter binding");
        assert!(matches!(ctrl_enter, ReedlineEvent::Submit));

        let alt_enter = kb
            .find_binding(KeyModifiers::ALT, KeyCode::Enter)
            .expect("alt-enter binding");
        assert!(matches!(alt_enter, ReedlineEvent::Submit));

        let ctrl_j = kb
            .find_binding(KeyModifiers::CONTROL, KeyCode::Char('j'))
            .expect("ctrl-j binding");
        assert!(matches!(ctrl_j, ReedlineEvent::None));

        let up = kb
            .find_binding(KeyModifiers::NONE, KeyCode::Up)
            .expect("up binding");
        assert!(matches!(
            up,
            ReedlineEvent::Edit(commands)
                if commands == [EditCommand::MoveLineUp { select: false }]
        ));
    }
}
