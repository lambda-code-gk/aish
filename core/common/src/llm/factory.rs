//! プロバイダファクトリー
//!
//! プロバイダタイプに基づいて適切なプロバイダを作成します。

use crate::error::Error;
use crate::llm::driver::LlmDriver;
use crate::llm::echo::EchoProvider;
use crate::llm::gemini::GeminiProvider;
use crate::llm::gpt::GptProvider;
use crate::llm::openai_compat::OpenAiCompatProvider;
use crate::llm::provider::{LlmProvider, Message};
use crate::tool::ToolDef;
use serde_json::Value;

/// プロバイダタイプ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    /// Gemini 3 Flash
    Gemini,
    /// GPT
    Gpt,
    /// OpenAI Chat Completions 互換 (/chat/completions)
    OpenAiCompat,
    /// Echo（クエリを表示するだけ）
    Echo,
}

impl ProviderType {
    /// 文字列からプロバイダタイプを解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gemini" => Some(Self::Gemini),
            "gpt" | "openai" => Some(Self::Gpt),
            "openai_compat" => Some(Self::OpenAiCompat),
            "echo" => Some(Self::Echo),
            _ => None,
        }
    }

    /// プロバイダタイプを文字列に変換
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Gpt => "gpt",
            Self::OpenAiCompat => "openai_compat",
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
    OpenAiCompat(OpenAiCompatProvider),
    Echo(EchoProvider),
}

impl LlmProvider for AnyProvider {
    fn name(&self) -> &str {
        match self {
            Self::Gemini(p) => p.name(),
            Self::Gpt(p) => p.name(),
            Self::OpenAiCompat(p) => p.name(),
            Self::Echo(p) => p.name(),
        }
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, Error> {
        match self {
            Self::Gemini(p) => p.make_http_request(request_json),
            Self::Gpt(p) => p.make_http_request(request_json),
            Self::OpenAiCompat(p) => p.make_http_request(request_json),
            Self::Echo(p) => p.make_http_request(request_json),
        }
    }

    fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, Error> {
        match self {
            Self::Gemini(p) => p.parse_response_text(response_json),
            Self::Gpt(p) => p.parse_response_text(response_json),
            Self::OpenAiCompat(p) => p.parse_response_text(response_json),
            Self::Echo(p) => p.parse_response_text(response_json),
        }
    }

    fn check_tool_calls(&self, response_json: &str) -> Result<bool, Error> {
        match self {
            Self::Gemini(p) => p.check_tool_calls(response_json),
            Self::Gpt(p) => p.check_tool_calls(response_json),
            Self::OpenAiCompat(p) => p.check_tool_calls(response_json),
            Self::Echo(p) => p.check_tool_calls(response_json),
        }
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
        tools: Option<&[ToolDef]>,
    ) -> Result<Value, Error> {
        match self {
            Self::Gemini(p) => p.make_request_payload(query, system_instruction, history, tools),
            Self::Gpt(p) => p.make_request_payload(query, system_instruction, history, tools),
            Self::OpenAiCompat(p) => p.make_request_payload(query, system_instruction, history, tools),
            Self::Echo(p) => p.make_request_payload(query, system_instruction, history, tools),
        }
    }

    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        match self {
            Self::Gemini(p) => p.make_http_streaming_request(request_json, callback),
            Self::Gpt(p) => p.make_http_streaming_request(request_json, callback),
            Self::OpenAiCompat(p) => p.make_http_streaming_request(request_json, callback),
            Self::Echo(p) => p.make_http_streaming_request(request_json, callback),
        }
    }

    fn stream_events(
        &self,
        request_json: &str,
        tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(crate::llm::events::LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        match self {
            Self::Gemini(p) => p.stream_events(request_json, tools, callback),
            Self::Gpt(p) => p.stream_events(request_json, tools, callback),
            Self::OpenAiCompat(p) => p.stream_events(request_json, tools, callback),
            Self::Echo(p) => p.stream_events(request_json, tools, callback),
        }
    }
}

/// プロバイダを作成する
///
/// # Arguments
/// * `provider_type` - プロバイダタイプ
/// * `model` - モデル名（オプション、デフォルト値が使用される）
/// * `base_url` - ベース URL（Gpt / OpenAiCompat 用。None のとき各プロバイダのデフォルト）
/// * `api_key_env` - API キーを読む環境変数名（Gpt / OpenAiCompat 用。None のとき各プロバイダのデフォルト）
/// * `temperature` - 温度（Gpt / OpenAiCompat 用。None のとき各プロバイダのデフォルト）
pub fn create_provider(
    provider_type: ProviderType,
    model: Option<String>,
    base_url: Option<String>,
    api_key_env: Option<String>,
    temperature: Option<f32>,
) -> Result<AnyProvider, Error> {
    match provider_type {
        ProviderType::Gemini => {
            let provider = GeminiProvider::new(model)?;
            Ok(AnyProvider::Gemini(provider))
        }
        ProviderType::Gpt => {
            let provider = GptProvider::new(
                model,
                temperature.map(|t| t as f64),
                base_url,
                api_key_env,
            )?;
            Ok(AnyProvider::Gpt(provider))
        }
        ProviderType::OpenAiCompat => {
            let provider = OpenAiCompatProvider::new(model, base_url, api_key_env, temperature)?;
            Ok(AnyProvider::OpenAiCompat(provider))
        }
        ProviderType::Echo => {
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
/// 互換維持のため temperature=None で create_provider を呼ぶ。
/// ResolvedProvider の base_url / api_key_env / temperature を反映する場合は
/// `create_provider(..., resolved.base_url.clone(), resolved.api_key_env.clone(), resolved.temperature)` のあと
/// `LlmDriver::new(provider)` でドライバを組み立てる。
pub fn create_driver(
    provider_type: ProviderType,
    model: Option<String>,
) -> Result<LlmDriver<AnyProvider>, Error> {
    let provider = create_provider(provider_type, model, None, None, None)?;
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
        assert_eq!(ProviderType::from_str("openai_compat"), Some(ProviderType::OpenAiCompat));
        assert_eq!(ProviderType::from_str("echo"), Some(ProviderType::Echo));
        assert_eq!(ProviderType::from_str("ECHO"), Some(ProviderType::Echo));
        assert_eq!(ProviderType::from_str("unknown"), None);
    }

    #[test]
    fn test_provider_type_as_str() {
        assert_eq!(ProviderType::Gemini.as_str(), "gemini");
        assert_eq!(ProviderType::Gpt.as_str(), "gpt");
        assert_eq!(ProviderType::OpenAiCompat.as_str(), "openai_compat");
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

