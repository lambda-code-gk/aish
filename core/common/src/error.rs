//! エラーハンドリング
//!
//! ドメイン・ユースケース層では `Result<T, Error>` を使い、
//! CLI 境界（main）で `Error::exit_code()` により終了コードに変換する。

use std::io;

/// アプリケーション統一エラー型（enum）
///
/// 失敗を型で表現し、`match` でハンドリング可能。
/// 終了コードへの変換は CLI 境界でのみ行う。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 引数不正（終了コード 64）
    #[error("{0}")]
    InvalidArgument(String),

    /// I/O エラー（終了コード 74）
    #[error("{message}")]
    Io {
        message: String,
        #[source]
        source: Option<io::Error>,
    },

    /// 環境変数未設定・不正（終了コード 64）
    #[error("{0}")]
    Env(String),

    /// プロバイダ設定・不明プロバイダ（終了コード 64）
    #[error("{0}")]
    Provider(String),

    /// HTTP / API 通信エラー（終了コード 74）
    #[error("{0}")]
    Http(String),

    /// JSON 解析・プロトコルエラー（終了コード 74）
    #[error("{0}")]
    Json(String),

    /// タスク未検出（終了コード 64）
    #[error("{0}")]
    TaskNotFound(String),

    /// システム・シグナル等（終了コード 70）
    #[error("{0}")]
    System(String),

    /// 問題解決のための補足情報付きエラー（終了コード・is_usage は source に準拠）
    #[error("{context}\n  {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    /// CLI 境界で使用: 終了コードを返す（BSD exit codes を参考）
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::InvalidArgument(_) | Error::Env(_) | Error::Provider(_) | Error::TaskNotFound(_) => 64,
            Error::System(_) => 70,
            Error::Io { .. } | Error::Http(_) | Error::Json(_) => 74,
            Error::WithContext { source, .. } => source.exit_code(),
        }
    }

    /// 用法表示すべきか（InvalidArgument / Env / Provider / TaskNotFound は true）
    pub fn is_usage(&self) -> bool {
        match self {
            Error::WithContext { source, .. } => source.is_usage(),
            _ => self.exit_code() == 64,
        }
    }

    /// 問題解決に役立つ補足情報を付与してエラーを包む
    pub fn with_context(self, context: impl Into<String>) -> Self {
        Error::WithContext {
            context: context.into(),
            source: Box::new(self),
        }
    }

    // --- コンストラクタ（従来のヘルパー相当）---

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Error::InvalidArgument(msg.into())
    }

    pub fn io_msg(msg: impl Into<String>) -> Self {
        Error::Io {
            message: msg.into(),
            source: None,
        }
    }

    pub fn env(msg: impl Into<String>) -> Self {
        Error::Env(msg.into())
    }

    pub fn provider(msg: impl Into<String>) -> Self {
        Error::Provider(msg.into())
    }

    pub fn http(msg: impl Into<String>) -> Self {
        Error::Http(msg.into())
    }

    pub fn json(msg: impl Into<String>) -> Self {
        Error::Json(msg.into())
    }

    pub fn task_not_found(msg: impl Into<String>) -> Self {
        Error::TaskNotFound(msg.into())
    }

    pub fn system(msg: impl Into<String>) -> Self {
        Error::System(msg.into())
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        let message = e.to_string();
        Error::Io {
            message: message.clone(),
            source: Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_exit_codes() {
        assert_eq!(Error::invalid_argument("test").exit_code(), 64);
        assert_eq!(Error::io_msg("test").exit_code(), 74);
        assert_eq!(Error::env("test").exit_code(), 64);
        assert_eq!(Error::http("test").exit_code(), 74);
        assert_eq!(Error::json("test").exit_code(), 74);
        assert_eq!(Error::system("test").exit_code(), 70);
    }

    #[test]
    fn test_is_usage() {
        assert!(Error::invalid_argument("x").is_usage());
        assert!(Error::env("x").is_usage());
        assert!(!Error::io_msg("x").is_usage());
        assert!(!Error::http("x").is_usage());
    }

    #[test]
    fn test_from_io_error() {
        let e = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: Error = e.into();
        assert_eq!(err.exit_code(), 74);
    }

    #[test]
    fn test_with_context_delegates_exit_code_and_usage() {
        let e = Error::http("connection refused").with_context("Provider profile: local (base_url: http://localhost:11434/v1)");
        assert_eq!(e.exit_code(), 74);
        assert!(!e.is_usage());
        let e2 = Error::invalid_argument("bad").with_context("Context");
        assert_eq!(e2.exit_code(), 64);
        assert!(e2.is_usage());
    }
}
