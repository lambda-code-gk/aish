//! AISH共通ライブラリ
//!
//! `ai`と`aish`コマンドで共有される機能を提供します。

/// アダプター（外界 I/O の trait と標準実装）
pub mod adapter;

/// ドメイン型（Newtype）
pub mod domain;

/// エラーハンドリング
pub mod error;

/// LLMドライバーとプロバイダ
pub mod llm;

/// 型付きメッセージ履歴（Msg）
pub mod msg;

/// Part ID生成（固定長・辞書順＝時系列）
pub mod part_id;

/// セッション管理
pub mod session;

/// イベント Sink（表示・保存の分離）
pub mod sink;

/// ツール実行（Ports & Adapters）
pub mod tool;
