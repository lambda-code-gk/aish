//! TruncateConsoleLog コマンドのユースケース

use common::ports::outbound::FileSystem;
use common::error::Error;
use common::ports::outbound::{PathResolver, PathResolverInput, Signal};
use common::session::Session;
use std::path::Path;
use std::sync::Arc;

/// TruncateConsoleLog コマンドのユースケース
pub struct TruncateConsoleLogUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
    signal: Arc<dyn Signal>,
}

impl TruncateConsoleLogUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
        signal: Arc<dyn Signal>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
            signal,
        }
    }

    /// TruncateConsoleLog を実行する
    pub fn run(&self, path_input: &PathResolverInput) -> Result<i32, Error> {
        let session = self.resolve_session(path_input)?;
        self.truncate_console_log(session.session_dir().as_ref())
    }

    fn resolve_session(&self, path_input: &PathResolverInput) -> Result<Session, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self.path_resolver.resolve_session_dir(path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }

    fn truncate_console_log(&self, session_dir: &Path) -> Result<i32, Error> {
        let pid_file_path = session_dir.join("AISH_PID");

        if !self.fs.exists(&pid_file_path) {
            return Ok(0);
        }

        let pid_str = self.fs.read_to_string(&pid_file_path)?;
        let pid: i32 = pid_str
            .trim()
            .parse()
            .map_err(|e| Error::io_msg(format!("Invalid PID in AISH_PID file: {}", e)))?;

        self.signal.send_signal(pid, libc::SIGUSR2)?;
        Ok(0)
    }
}
