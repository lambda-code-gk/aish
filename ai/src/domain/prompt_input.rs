//! 対話的プロンプト入力のドメイン型と送信前検証。

/// プロンプト取得の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAcquisitionResult {
    /// ユーザーが送信した非空テキスト
    Submitted { content: String },
    /// 空入力またはコメントのみ
    Empty,
    /// ユーザーがキャンセル（Ctrl+C 等）
    Cancelled,
    /// 外部エディタが異常終了した
    EditorFailed { exit_code: Option<i32> },
}

impl PromptAcquisitionResult {
    pub fn is_ready_for_ask(&self) -> bool {
        matches!(self, Self::Submitted { .. })
    }
}

/// 外部エディタ用テンプレートの HTML コメントブロックを除去する。
pub fn strip_prompt_template_comments(content: &str) -> String {
    let re = regex::Regex::new(r"(?s)<!--\s*ai-prompt:.*?-->").expect("valid regex");
    let stripped = re.replace_all(content, "");
    stripped
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// 送信前の実体があるか（空白のみは空扱い）。
pub fn is_substantive_prompt(content: &str) -> bool {
    !content.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ai_prompt_comment_block() {
        let raw = "<!-- ai-prompt: Enter your prompt below. -->\n\nhello";
        assert_eq!(strip_prompt_template_comments(raw), "hello");
    }

    #[test]
    fn comment_only_is_empty_after_strip() {
        let raw = "<!-- ai-prompt: hint only -->";
        assert!(!is_substantive_prompt(&strip_prompt_template_comments(raw)));
    }

    #[test]
    fn multiline_content_preserved() {
        let raw = "line 1\nline 2\nline 3";
        assert_eq!(strip_prompt_template_comments(raw), raw);
    }
}
