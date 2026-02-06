//! LLMドライバーとプロバイダの実装
//!
//! このモジュールは、異なるLLMプロバイダ（Gemini、GPTなど）で共通する処理を提供します。

pub mod config;
pub mod driver;
pub mod events;
pub mod provider;
pub mod gemini;
pub mod gpt;
pub mod echo;
pub mod factory;
pub mod openai_compat;
pub mod resolver;

pub use config::{ProfilesConfig, ProviderProfile, ProviderTypeKind};
pub use driver::LlmDriver;
pub use events::{FinishReason, LlmEvent};
pub use factory::{ProviderType, create_provider, create_driver};
pub use provider::LlmProvider;
pub use resolver::{load_profiles_config, resolve_provider, ResolvedProvider};

