//! aish ログ読み込み outbound port。

#[derive(Debug, thiserror::Error)]
pub enum LogReadError {
    #[error("failed to read log: {0}")]
    Read(String),
}

/// ログファイル末尾を取得する（aish クレートには依存しない）。
pub trait ShellLogSource {
    fn tail_bytes(&self, max_bytes: usize) -> Result<String, LogReadError>;
}
