//! Ctrl+C（SIGINT）等による割り込みを検知する Outbound ポート
//!
//! ストリーミング中にユーザーが中断した場合、状態を保存して終了し、次回 resume できるようにするために使用する。

/// 割り込みが要求されたかどうかを返す能力
///
/// usecase はストリームのコールバック内でこの trait を参照し、true なら Err を返して保存後に終了する。
pub trait InterruptChecker: Send + Sync {
    fn is_interrupted(&self) -> bool;
}
