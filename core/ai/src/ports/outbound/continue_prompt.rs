//! エージェントループ上限到達時に「続けますか？」をユーザーに問い合わせる Outbound ポート

use common::error::Error;

/// 上限到達時に続行するかどうかをユーザーに問い合わせる能力
///
/// usecase はこの trait にのみ依存し、adapter が stdin/stdout でプロンプトを表示する。
pub trait ContinueAfterLimitPrompt: Send + Sync {
    /// ユーザーに「続けますか？」を問い合わせ、続行するなら true、しないなら false を返す。
    fn ask_continue(&self) -> Result<bool, Error>;
}
