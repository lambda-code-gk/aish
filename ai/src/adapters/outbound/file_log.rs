//! ログファイル tail アダプタ。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use crate::ports::outbound::{LogReadError, ShellLogSource};

pub struct FileLogTail {
    path: PathBuf,
}

impl FileLogTail {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ShellLogSource for FileLogTail {
    fn tail_bytes(&self, max_bytes: usize) -> Result<String, LogReadError> {
        let mut file = File::open(&self.path).map_err(|e| LogReadError::Read(e.to_string()))?;
        let len = file
            .metadata()
            .map_err(|e| LogReadError::Read(e.to_string()))?
            .len();
        let start = len.saturating_sub(max_bytes as u64);
        file.seek(SeekFrom::Start(start))
            .map_err(|e| LogReadError::Read(e.to_string()))?;
        let read_len = (len - start) as usize;
        let mut buf = vec![0u8; read_len];
        file.read_exact(&mut buf)
            .map_err(|e| LogReadError::Read(e.to_string()))?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn tail_reads_only_suffix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("big.log");
        let payload = "x".repeat(32 * 1024);
        fs::write(&path, &payload).expect("write");

        let tail = FileLogTail::new(path);
        let got = tail.tail_bytes(1024).expect("tail");
        assert_eq!(got.len(), 1024);
        assert!(got.chars().all(|c| c == 'x'));
    }
}
