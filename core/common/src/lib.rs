//! AISH共通ライブラリ
//!
//! `ai`と`aish`コマンドで共有される機能を提供します。

/// アダプター（外界 I/O の標準実装；trait は ports に定義）
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

/// セッション配下パス検証（パストラバーサル対策）
pub mod safe_session_path;

/// Ports & Adapters のポート定義（inbound / outbound）
pub mod ports;

/// セッション管理
pub mod session;

/// システムプロンプト（sysq）のドメインとマージ
pub mod system_prompt;

/// イベント Sink（表示・保存の分離；trait は ports に定義）
pub mod sink;

/// ツール実行（ToolDef / ToolRegistry；Tool trait は ports に定義）
pub mod tool;

/// イベント配信 dispatcher（1回の emit で複数 sink へ配信）
pub mod event_hub;
