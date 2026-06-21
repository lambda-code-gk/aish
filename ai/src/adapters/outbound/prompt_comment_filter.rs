//! 外部エディタ下書きからテンプレート注釈を除去する adapter。

use crate::domain::strip_prompt_template_comments;

pub fn filter_editor_draft(content: &str) -> String {
    strip_prompt_template_comments(content)
}
