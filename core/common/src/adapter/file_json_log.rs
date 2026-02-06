//! ファイルへ JSONL で追記する Log 実装
//!
//! ログの出力先はファイルのみ。エラー時のコンソール表示（stderr）とは別。

use crate::error::Error;
use crate::ports::outbound::{FileSystem, Log, LogRecord};
use std::path::Path;
use std::sync::Arc;

/// ファイルへ JSONL を追記する Log 実装
pub struct FileJsonLog {
    fs: Arc<dyn FileSystem>,
    path: std::path::PathBuf,
}

impl FileJsonLog {
    /// ログファイルパスへ追記する logger を生成する。
    /// 親ディレクトリが無ければ作成する（初回書き込み時）。
    pub fn new(fs: Arc<dyn FileSystem>, path: impl AsRef<Path>) -> Self {
        Self {
            fs,
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl Log for FileJsonLog {
    fn log(&self, record: &LogRecord) -> Result<(), Error> {
        if let Some(parent) = self.path.parent() {
            self.fs.create_dir_all(parent)?;
        }
        let mut w = self.fs.open_append(&self.path)?;
        let line = serde_json::to_string(record).map_err(|e| Error::Json(e.to_string()))?;
        use std::io::Write;
        w.write_all(line.as_bytes())
            .map_err(|e| Error::io_msg(e.to_string()))?;
        w.write_all(b"\n")
            .map_err(|e| Error::io_msg(e.to_string()))?;
        w.flush().map_err(|e| Error::io_msg(e.to_string()))?;
        Ok(())
    }
}

/// 何も出力しない Log 実装（テスト用）
#[derive(Debug, Clone, Default)]
pub struct NoopLog;

impl Log for NoopLog {
    fn log(&self, _record: &LogRecord) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::{now_iso8601, LogLevel};

    #[test]
    fn test_noop_log() {
        let log = NoopLog;
        let rec = LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "test".to_string(),
            layer: None,
            kind: None,
            fields: None,
        };
        assert!(log.log(&rec).is_ok());
    }
}
