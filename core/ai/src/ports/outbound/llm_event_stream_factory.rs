//! LLM イベントストリーム生成 Outbound ポート
//!
//! usecase はこの trait 経由でプロバイダ解決・ドライバ生成を行い、LlmEventStream を取得する。

use common::domain::{ModelName, ProviderName, SessionDir};
use common::error::Error;
use std::sync::Arc;

use super::LlmEventStream;

/// ストリーム作成時に返すエラー表示用コンテキスト（プロファイル名等）
#[derive(Clone)]
pub struct LlmStreamContext(pub String);

/// プロバイダ解決とドライバ生成を行い、LlmEventStream を返す Outbound ポート
pub trait LlmEventStreamFactory: Send + Sync {
    /// 指定の provider/model でストリームとエラー表示用コンテキストを生成する
    fn create_stream(
        &self,
        session_dir: Option<&SessionDir>,
        provider: Option<&ProviderName>,
        model: Option<&ModelName>,
        system_instruction: Option<&str>,
    ) -> Result<(Arc<dyn LlmEventStream>, LlmStreamContext), Error>;
}
