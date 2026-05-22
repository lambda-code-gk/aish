//! Unix 向けの二重 fork によるデーモン化。

use std::ffi::CString;
use std::io;

/// 標準的なデーモン化手順（fork → setsid → fork → chdir → umask → stdio を /dev/null へ）。
pub fn daemonize() -> io::Result<()> {
    unsafe {
        match libc::fork() {
            -1 => return Err(io::Error::last_os_error()),
            pid if pid > 0 => libc::_exit(0),
            _ => {}
        }

        if libc::setsid() == -1 {
            return Err(io::Error::last_os_error());
        }

        match libc::fork() {
            -1 => return Err(io::Error::last_os_error()),
            pid if pid > 0 => libc::_exit(0),
            _ => {}
        }

        if libc::chdir(CString::new("/").unwrap().as_ptr()) == -1 {
            return Err(io::Error::last_os_error());
        }

        // 作成ファイルのグループ/その他権限を抑える（socket 等）
        libc::umask(0o077);

        let devnull = CString::new("/dev/null").unwrap();
        let fd = libc::open(devnull.as_ptr(), libc::O_RDWR);
        if fd == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::dup2(fd, libc::STDIN_FILENO) == -1
            || libc::dup2(fd, libc::STDOUT_FILENO) == -1
            || libc::dup2(fd, libc::STDERR_FILENO) == -1
        {
            libc::close(fd);
            return Err(io::Error::last_os_error());
        }
        if fd > libc::STDERR_FILENO {
            libc::close(fd);
        }
    }

    Ok(())
}
