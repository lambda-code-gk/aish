//! プロバイダファクトリー
//!
//! プロバイダタイプに基づいて適切なプロバイダを作成します。

use crate::llm::driver::LlmDriver;
use crate::llm::gemini::GeminiProvider;
use crate::llm::gpt::GptProvider;
use crate::llm::echo::EchoProvider;
use crate::llm::provider::{LlmProvider, Message};
use serde_json::Value;

/// プロバイダタイプ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    /// Gemini 3 Flash
    Gemini,
    /// GPT
    Gpt,
    /// Echo（クエリを表示するだけ）
    Echo,
}

impl ProviderType {
    /// 文字列からプロバイダタイプを解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gemini" => Some(Self::Gemini),
            "gpt" | "openai" => Some(Self::Gpt),
            "echo" => Some(Self::Echo),
            _ => None,
        }
    }
    
    /// プロバイダタイプを文字列に変換
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Gpt => "gpt",
            Self::Echo => "echo",
        }
    }
}

/// プロバイダのenumラッパー
/// 
/// 異なるプロバイダタイプを型安全に扱うために使用します。
pub enum AnyProvider {
    Gemini(GeminiProvider),
    Gpt(GptProvider),
    Echo(EchoProvider),
}

impl LlmProvider for AnyProvider {
    fn name(&self) -> &str {
        match self {
            Self::Gemini(p) => p.name(),
            Self::Gpt(p) => p.name(),
            Self::Echo(p) => p.name(),
        }
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, (String, i32)> {
        match self {
            Self::Gemini(p) => p.make_http_request(request_json),
            Self::Gpt(p) => p.make_http_request(request_json),
            Self::Echo(p) => p.make_http_request(request_json),
        }
    }

    fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, (String, i32)> {
        match self {
            Self::Gemini(p) => p.parse_response_text(response_json),
            Self::Gpt(p) => p.parse_response_text(response_json),
            Self::Echo(p) => p.parse_response_text(response_json),
        }
    }

    fn check_tool_calls(&self, response_json: &str) -> Result<bool, (String, i32)> {
        match self {
            Self::Gemini(p) => p.check_tool_calls(response_json),
            Self::Gpt(p) => p.check_tool_calls(response_json),
            Self::Echo(p) => p.check_tool_calls(response_json),
        }
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<Value, (String, i32)> {
        match self {
            Self::Gemini(p) => p.make_request_payload(query, system_instruction, history),
            Self::Gpt(p) => p.make_request_payload(query, system_instruction, history),
            Self::Echo(p) => p.make_request_payload(query, system_instruction, history),
        }
    }

    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), (String, i32)>>,
    ) -> Result<(), (String, i32)> {
        match self {
            Self::Gemini(p) => p.make_http_streaming_request(request_json, callback),
            Self::Gpt(p) => p.make_http_streaming_request(request_json, callback),
            Self::Echo(p) => p.make_http_streaming_request(request_json, callback),
        }
    }
}

/// プロバイダを作成する
/// 
/// # Arguments
/// * `provider_type` - プロバイダタイプ
/// * `model` - モデル名（オプション、デフォルト値が使用される）
/// 
/// # Returns
/// * `Ok(AnyProvider)` - プロバイダ
/// * `Err((String, i32))` - エラーメッセージと終了コード
pub fn create_provider(
    provider_type: ProviderType,
    model: Option<String>,
) -> Result<AnyProvider, (String, i32)> {
    match provider_type {
        ProviderType::Gemini => {
            let provider = GeminiProvider::new(model)?;
            Ok(AnyProvider::Gemini(provider))
        }
        ProviderType::Gpt => {
            let provider = GptProvider::new(model, None)?;
            Ok(AnyProvider::Gpt(provider))
        }
        ProviderType::Echo => {
            // Echoプロバイダはモデルを無視
            let provider = EchoProvider::new();
            Ok(AnyProvider::Echo(provider))
        }
    }
}

/// ドライバーを作成する
/// 
/// # Arguments
/// * `provider_type` - プロバイダタイプ
/// * `model` - モデル名（オプション、デフォルト値が使用される）
/// 
/// # Returns
/// * `Ok(LlmDriver<AnyProvider>)` - ドライバー
/// * `Err((String, i32))` - エラーメッセージと終了コード
pub fn create_driver(
    provider_type: ProviderType,
    model: Option<String>,
) -> Result<LlmDriver<AnyProvider>, (String, i32)> {
    let provider = create_provider(provider_type, model)?;
    Ok(LlmDriver::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_type_from_str() {
        assert_eq!(ProviderType::from_str("gemini"), Some(ProviderType::Gemini));
        assert_eq!(ProviderType::from_str("Gemini"), Some(ProviderType::Gemini));
        assert_eq!(ProviderType::from_str("GEMINI"), Some(ProviderType::Gemini));
        assert_eq!(ProviderType::from_str("gpt"), Some(ProviderType::Gpt));
        assert_eq!(ProviderType::from_str("GPT"), Some(ProviderType::Gpt));
        assert_eq!(ProviderType::from_str("openai"), Some(ProviderType::Gpt));
        assert_eq!(ProviderType::from_str("echo"), Some(ProviderType::Echo));
        assert_eq!(ProviderType::from_str("ECHO"), Some(ProviderType::Echo));
        assert_eq!(ProviderType::from_str("unknown"), None);
    }

    #[test]
    fn test_provider_type_as_str() {
        assert_eq!(ProviderType::Gemini.as_str(), "gemini");
        assert_eq!(ProviderType::Gpt.as_str(), "gpt");
        assert_eq!(ProviderType::Echo.as_str(), "echo");
    }

    #[test]
    fn test_any_provider_name_gemini() {
        // 実際のAPIキーが必要なため、統合テストで確認
        // ここでは基本的な構造のテストのみ
        // 環境変数を設定してテストする場合は、統合テストで行う
    }

    #[test]
    fn test_any_provider_name_gpt() {
        // 実際のAPIキーが必要なため、統合テストで確認
        // ここでは基本的な構造のテストのみ
    }
}

