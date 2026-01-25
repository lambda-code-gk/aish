//! エラーハンドリング
//!
//! プロジェクト全体で統一されたエラーハンドリングを提供します。
//! エラーは`(エラーメッセージ, 終了コード)`の形式で統一されています。

/// エラー型
/// 
/// `(エラーメッセージ, 終了コード)`の形式で統一
pub type Error = (String, i32);

/// エラーハンドリングのヘルパー関数

/// I/Oエラーをエラー型に変換
/// 
/// # Arguments
/// * `msg` - エラーメッセージ
/// * `code` - 終了コード（デフォルト: 74）
pub fn io_error(msg: &str, code: i32) -> Error {
    (msg.to_string(), code)
}

/// I/Oエラーをエラー型に変換（デフォルトの終了コード74を使用）
pub fn io_error_default(msg: &str) -> Error {
    io_error(msg, 74)
}

/// 引数不正エラー（終了コード: 64）
pub fn invalid_argument(msg: &str) -> Error {
    (msg.to_string(), 64)
}

/// システムエラー（終了コード: 70）
pub fn system_error(msg: &str) -> Error {
    (msg.to_string(), 70)
}

/// HTTPエラー（終了コード: 74）
pub fn http_error(msg: &str) -> Error {
    (msg.to_string(), 74)
}

/// JSON解析エラー（終了コード: 74）
pub fn json_error(msg: &str) -> Error {
    (msg.to_string(), 74)
}

/// 環境変数エラー（終了コード: 64）
pub fn env_error(msg: &str) -> Error {
    (msg.to_string(), 64)
}

/// 汎用エラー作成関数
/// 
/// # Arguments
/// * `msg` - エラーメッセージ
/// * `code` - 終了コード
pub fn error(msg: &str, code: i32) -> Error {
    (msg.to_string(), code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_helpers() {
        let err = invalid_argument("test");
        assert_eq!(err.0, "test");
        assert_eq!(err.1, 64);
        
        let err = system_error("test");
        assert_eq!(err.0, "test");
        assert_eq!(err.1, 70);
    }
}

