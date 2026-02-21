//! システムプロンプトをフックから解決する Outbound ポート
//!
//! -S 未指定時に、プロジェクト・ユーザー・システムの各 hooks/system_prompt を実行し、
//! 標準出力を合成してシステムプロンプトとして返す。

use common::error::Error;

/// フックを実行してシステムプロンプトを解決する能力
///
/// 3 種類のフックディレクトリ（システム → ユーザー → プロジェクト）を順に実行し、
/// 各 stdout を `\n\n` で結合する。いずれも無い／全て空の場合は None。
pub trait ResolveSystemPromptFromHooks: Send + Sync {
    fn resolve_system_prompt_from_hooks(&self) -> Result<Option<String>, Error>;
}
