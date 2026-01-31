//! LLMドライバーとプロバイダの実装
//!
//! このモジュールは、異なるLLMプロバイダ（Gemini、GPTなど）で共通する処理を提供します。

pub mod driver;
pub mod events;
pub mod provider;
pub mod gemini;
pub mod gpt;
pub mod echo;
pub mod factory;

pub use driver::LlmDriver;
pub use events::{FinishReason, LlmEvent};
pub use provider::LlmProvider;
pub use factory::{ProviderType, create_provider, create_driver};

