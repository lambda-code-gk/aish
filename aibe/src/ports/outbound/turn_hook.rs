//! Agent turn 前処理への割り込み（memory pack 等）。

use crate::domain::{AgentTurnContext, ChatMessage};

/// `TurnHook` 実行時の失敗。呼び出し側は turn 全体を落とさず best-effort で継続する。
#[derive(Debug, Clone, thiserror::Error)]
#[error("turn hook failed: {0}")]
pub struct TurnHookError(pub String);

/// `agent_turn` の prompt 組み立てに割り込む trait（memory 注入等）。
pub trait TurnHook: Send + Sync {
    /// system instruction / shell log tail を前置した後の messages に対し、追加コンテキストを注入する。
    fn prepare_turn_messages(
        &self,
        context: &AgentTurnContext,
        messages: Vec<ChatMessage>,
    ) -> Result<Vec<ChatMessage>, TurnHookError>;
}
