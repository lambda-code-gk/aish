//! Unix 向けデーモン化と process lifecycle primitive。

mod pid_file;
mod process;

pub use pid_file::{
    build_current_pid_record, cleanup_runtime_artifacts, cleanup_stale_pid_file_before_start,
    current_process_start_jiffies, default_pid_file_path, default_pid_file_path_for_home,
    is_trusted_runtime_socket, read_pid_file, remove_pid_file, remove_trusted_runtime_socket,
    runtime_share_dir, validate_pid_record, validate_pid_record_for_paths, write_pid_file,
    PidFileError, PidFileRecord, PidFileState,
};
pub use process::{
    send_sigkill, send_sigterm, wait_for_process_exit, ProcessError, DEFAULT_STOP_WAIT,
};

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
