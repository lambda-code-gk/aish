// Unix系プラットフォーム（Linux/macOS）向けのファイル監視実装
// 現在はポーリング方式を実装。将来的にinotify/kqueue対応を追加可能。

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// ファイル監視のためのハンドル
pub struct FileWatcher {
    path: PathBuf,
    position: u64,
    poll_interval_ms: u64,
}

impl FileWatcher {
    /// 新しいFileWatcherを作成
    pub fn new<P: AsRef<Path>>(path: P, poll_interval_ms: u64) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        // 初期位置を設定（ファイルが存在する場合、末尾から）
        let position = if path.exists() {
            let file = File::open(&path)?;
            file.metadata()?.len()
        } else {
            0
        };
        Ok(Self {
            path,
            position,
            poll_interval_ms,
        })
    }

    /// ファイルの末尾から読み取りを開始（tail -f相当）
    pub fn seek_to_end(&mut self) -> std::io::Result<()> {
        if self.path.exists() {
            let file = File::open(&self.path)?;
            self.position = file.metadata()?.len();
        }
        Ok(())
    }

    /// ファイルの新しい行を読み取る（ポーリング方式）
    /// 複数の行が追加されている場合、それらをすべて読み取る
    pub fn read_new_lines(&mut self) -> std::io::Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path)?;
        let current_size = file.metadata()?.len();
        
        if current_size < self.position {
            // ファイルが小さくなった = ローテーションされた
            self.position = 0;
        }
        
        if current_size <= self.position {
            // 新しいデータなし
            return Ok(Vec::new());
        }
        
        // 新しい部分を読み取る
        let mut file = file;
        file.seek(SeekFrom::Start(self.position))?;
        let mut reader = BufReader::new(file);
        let mut lines = Vec::new();
        
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    self.position += n as u64;
                    if !line.is_empty() {
                        lines.push(line);
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        
        Ok(lines)
    }

    /// ポーリング間隔を取得
    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }
}
