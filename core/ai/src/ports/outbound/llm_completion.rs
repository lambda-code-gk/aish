//! 単発 LLM 完了の Outbound ポート
//!
//! ストリーミングではなく 1 回のプロンプトで全文応答を取得する（自己改善の知見抽出などで利用）。

use common::error::Error;

/// 単発の LLM 完了（system + user で応答文字列を取得）
pub trait LlmCompletion: Send + Sync {
    fn complete(&self, system_instruction: Option<&str>, user_message: &str) -> Result<String, Error>;
}
