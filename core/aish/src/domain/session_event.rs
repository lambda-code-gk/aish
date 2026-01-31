//! セッションイベント（シグナル由来）
//!
//! SIGUSR1/2, SIGWINCH をイベントに変換し、ハンドラで集約処理する。

/// シェルセッションで扱うシグナル由来のイベント
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    /// SIGUSR1: バッファをログにフラッシュし、rollover
    SigUsr1,
    /// SIGUSR2: バッファをクリアし、ログを truncate
    SigUsr2,
    /// SIGWINCH: ウィンドウサイズ変更（PTY に伝搬）
    SigWinch,
}
