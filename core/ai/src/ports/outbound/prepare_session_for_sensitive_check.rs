//! セッションの機微チェック準備 Outbound ポート
//!
//! 未処理の part を leakscan し、ヒット時はユーザーに問い合わせ、
//! reviewed 作成・退避移動を行う。usecase はこの trait 経由でのみ呼ぶ。

use common::domain::SessionDir;
use common::error::Error;

/// セッション dir 内の未処理 part を leakscan し、reviewed 作成・退避を行う能力
///
/// prepare 後、履歴は reviewed_* のみから構築する実装で load すればよい。
pub trait PrepareSessionForSensitiveCheck: Send + Sync {
    /// セッション dir 内の part_* を処理する（leakscan、問い合わせ、reviewed 作成・退避移動）。
    fn prepare(&self, session_dir: &SessionDir) -> Result<(), Error>;
}
