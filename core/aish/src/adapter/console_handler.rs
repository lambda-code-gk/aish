//! コンソールログのイベントハンドラ（状態機械）
//!
//! SessionEvent を受け取り、flush / rollover / truncate の責務を集約する。
//! shell はイベント取得のみ行い、処理はここに委譲する。

use crate::domain::SessionEvent;
use common::error::Error;
use common::ports::outbound::FileSystem;
use common::part_id::IdGenerator;
use std::io::Write;
use std::path::Path;

/// part ファイル名を生成（テスト・shell から使用）
pub(crate) fn part_filename_from_id(id: &common::domain::PartId) -> String {
    format!("part_{}_user.txt", id)
}

fn rollover_log_file<F: FileSystem + ?Sized, I: IdGenerator + ?Sized>(
    log_file_path: &Path,
    session_dir: &Path,
    fs: &F,
    id_gen: &I,
) -> Result<(), Error> {
    if fs.exists(log_file_path) {
        let metadata = fs.metadata(log_file_path)?;
        if metadata.len() > 0 {
            let part_filename = part_filename_from_id(&id_gen.next_id());
            let part_file_path = session_dir.join(&part_filename);
            fs.rename(log_file_path, &part_file_path)?;
        }
    }
    fs.truncate_file(log_file_path)?;
    Ok(())
}

/// コンソールログのイベントを処理するハンドラ
pub struct ConsoleLogHandler<'a, F: ?Sized, I: ?Sized> {
    log_file_path: &'a Path,
    session_dir: &'a Path,
    fs: &'a F,
    id_gen: &'a I,
}

impl<'a, F: FileSystem + ?Sized, I: IdGenerator + ?Sized> ConsoleLogHandler<'a, F, I> {
    pub fn new(log_file_path: &'a Path, session_dir: &'a Path, fs: &'a F, id_gen: &'a I) -> Self {
        Self {
            log_file_path,
            session_dir,
            fs,
            id_gen,
        }
    }

    /// イベントを処理し、新しいログファイルハンドルを返す。
    /// 呼び出し元は返されたハンドルで log_file を更新する。
    pub fn handle(
        &self,
        event: SessionEvent,
        buffer_output: &str,
        mut log_file: Box<dyn Write + Send>,
    ) -> Result<Box<dyn Write + Send>, Error> {
        // ミュートフラグ（console.muted）が存在する場合は、console.txt への記録や
        // part ファイルへのロールオーバー / truncate を行わない。
        let muted = {
            let mute_flag_path = self.session_dir.join("console.muted");
            self.fs.exists(&mute_flag_path)
        };

        match event {
            SessionEvent::SigUsr1 => {
                if muted {
                    // ミュート中はバッファのフラッシュやロールオーバーを行わない
                    return Ok(log_file);
                } else {
                    if !buffer_output.is_empty() {
                        log_file.write_all(buffer_output.as_bytes())?;
                        log_file.flush()?;
                    }
                    drop(log_file);
                    rollover_log_file(self.log_file_path, self.session_dir, self.fs, self.id_gen)?;
                }
            }
            SessionEvent::SigUsr2 => {
                if muted {
                    // ミュート中は truncate も行わない
                    return Ok(log_file);
                } else {
                    drop(log_file);
                    self.fs.truncate_file(self.log_file_path)?;
                }
            }
            SessionEvent::SigWinch => {
                // PTY winsize は shell 側で処理（ここでは何もしない）
                return Ok(log_file);
            }
        }
        self.fs.open_append(self.log_file_path)
    }
}
