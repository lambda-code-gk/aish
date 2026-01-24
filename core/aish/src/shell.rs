use std::process;
use std::env;
use common::session::Session;

pub fn run_shell(session: &Session) -> Result<i32, (String, i32)> {
    // 環境変数SHELLを確認、なければbashをデフォルトとして使用
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    
    // コマンドを構築
    let mut cmd = process::Command::new(&shell);
    
    // AISH_SESSION環境変数を設定（セッションディレクトリのパス）
    cmd.env("AISH_SESSION", session.session_dir());
    
    // AISH_HOME環境変数を設定（ホームディレクトリのパス）
    cmd.env("AISH_HOME", session.aish_home());
    
    // シェルをコプロセスとして実行
    let mut child = cmd
        .spawn()
        .map_err(|e| {
            (
                format!("Failed to spawn shell '{}': {}", shell, e),
                70, // システムエラー
            )
        })?;
    
    // シェルの終了を待つ
    let exit_status = child.wait().map_err(|e| {
        (
            format!("Failed to wait for shell process: {}", e),
            70, // システムエラー
        )
    })?;
    
    // シェルの終了コードを返す
    Ok(exit_status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_detection() {
        // 環境変数SHELLの確認は統合テストで行う
        // ここでは基本的な構造のテストのみ
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        assert!(!shell.is_empty());
    }
}

