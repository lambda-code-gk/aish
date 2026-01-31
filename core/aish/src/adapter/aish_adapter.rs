//! aish 用アダプター実装（common の Pty / Signal を platform/unix で実装）

#[cfg(unix)]
mod unix {
    use common::adapter::{Pty, PtyProcessStatus, PtySpawn, Signal, Winsize};
    use common::error::Error;
    use std::os::unix::io::RawFd;

    use crate::adapter::platform::{self, ProcessStatus};

    fn to_common_status(s: ProcessStatus) -> PtyProcessStatus {
        match s {
            ProcessStatus::Exited(code) => PtyProcessStatus::Exited(code),
            ProcessStatus::Signaled(sig) => PtyProcessStatus::Signaled(sig),
        }
    }

    fn to_libc_winsize(ws: &Winsize) -> libc::winsize {
        libc::winsize {
            ws_row: ws.ws_row,
            ws_col: ws.ws_col,
            ws_xpixel: ws.ws_xpixel,
            ws_ypixel: ws.ws_ypixel,
        }
    }

    /// platform::unix::Pty を common::adapter::Pty としてラップ
    pub struct UnixPty(pub platform::Pty);

    impl Pty for UnixPty {
        fn master_fd(&self) -> RawFd {
            self.0.master_fd()
        }

        fn wait_nonblocking(&self) -> Result<Option<PtyProcessStatus>, Error> {
            self.0
                .wait_nonblocking()
                .map(|opt| opt.map(to_common_status))
                .map_err(|e| Error::io_msg(format!("waitpid failed: {}", e)))
        }

        fn set_winsize(&self, ws: &Winsize) -> Result<(), Error> {
            self.0
                .set_winsize(to_libc_winsize(ws))
                .map_err(|e| Error::io_msg(format!("set_winsize failed: {}", e)))
        }
    }

    /// common::adapter::PtySpawn の Unix 実装
    #[derive(Debug, Clone, Copy, Default)]
    pub struct UnixPtySpawn;

    impl PtySpawn for UnixPtySpawn {
        fn spawn(
            &self,
            cmd: Option<&[String]>,
            cwd: Option<&str>,
            env: &[(String, String)],
        ) -> Result<Box<dyn Pty>, Error> {
            let pty = platform::Pty::new(cmd, cwd, env)
                .map_err(|e| Error::system(format!("Failed to create PTY: {}", e)))?;
            Ok(Box::new(UnixPty(pty)))
        }
    }

    /// common::adapter::Signal の Unix 実装（platform の setup_* / check_* / libc::kill）
    #[derive(Debug, Clone, Copy, Default)]
    pub struct UnixSignal;

    impl Signal for UnixSignal {
        fn setup_sigwinch(&self) -> Result<(), Error> {
            platform::setup_sigwinch()
                .map_err(|e| Error::system(format!("Failed to setup SIGWINCH: {}", e)))
        }

        fn setup_sigusr1(&self) -> Result<(), Error> {
            platform::setup_sigusr1()
                .map_err(|e| Error::system(format!("Failed to setup SIGUSR1: {}", e)))
        }

        fn setup_sigusr2(&self) -> Result<(), Error> {
            platform::setup_sigusr2()
                .map_err(|e| Error::system(format!("Failed to setup SIGUSR2: {}", e)))
        }

        fn check_sigwinch(&self) -> bool {
            platform::check_sigwinch()
        }

        fn check_sigusr1(&self) -> bool {
            platform::check_sigusr1()
        }

        fn check_sigusr2(&self) -> bool {
            platform::check_sigusr2()
        }

        fn send_signal(&self, pid: i32, sig: i32) -> Result<(), Error> {
            unsafe {
                let result = libc::kill(pid, sig);
                if result != 0 {
                    let err = std::io::Error::last_os_error();
                    if err.raw_os_error() != Some(libc::ESRCH) {
                        return Err(Error::io_msg(format!(
                            "Failed to send signal to process: {}",
                            err
                        )));
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(unix)]
pub use unix::{UnixPtySpawn, UnixSignal};
