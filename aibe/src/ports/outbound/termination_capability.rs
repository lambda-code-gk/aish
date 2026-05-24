//! max-round 終端戦略選択に使うプロバイダ能力（読み取り専用 metadata）。

/// LLM adapter / composition root が提供する能力表現。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminationCapability {
    /// plain `complete()`（tools なし）で `role: tool` を送っても provider が解釈する。
    pub plain_complete_accepts_tool_role: bool,
}

impl TerminationCapability {
    pub fn summary_prompt_only() -> Self {
        Self {
            plain_complete_accepts_tool_role: false,
        }
    }
}
