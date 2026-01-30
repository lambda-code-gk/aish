//! AISH共通ライブラリ
//!
//! `ai`と`aish`コマンドで共有される機能を提供します。

/// エラーハンドリング
pub mod error;

/// セッション管理
pub mod session;

/// LLMドライバーとプロバイダ
pub mod llm;

/// Part ID生成（固定長・辞書順＝時系列）
pub mod part_id;
