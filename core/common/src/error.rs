//! エラーハンドリング

/// エラー型
/// 
/// `(エラーメッセージ, 終了コード)`の形式で統一
pub type Error = (String, i32);

/// エラーハンドリングのヘルパー関数

/// I/Oエラーをエラー型に変換
pub fn io_error(msg: &str, code: i32) -> Error {
    (msg.to_string(), code)
}

/// 引数不正エラー
pub fn invalid_argument(msg: &str) -> Error {
    (msg.to_string(), 64)
}

/// システムエラー
pub fn system_error(msg: &str) -> Error {
    (msg.to_string(), 70)
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

