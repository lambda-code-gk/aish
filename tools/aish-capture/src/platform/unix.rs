use libc::{self, c_int, pid_t};
use std::ffi::CString;
use std::io;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
#[link(name = "util")]
extern "C" {
    fn forkpty(
        amaster: *mut c_int,
        name: *const libc::c_char,
        termp: *const libc::termios,
        winp: *const libc::winsize,
    ) -> pid_t;
}

#[cfg(target_os = "macos")]
extern "C" {
    fn forkpty(
        amaster: *mut c_int,
        name: *const libc::c_char,
        termp: *const libc::termios,
        winp: *const libc::winsize,
    ) -> pid_t;
}

static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);
static SIGUSR1_RECEIVED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigwinch_handler(_signum: c_int) {
    SIGWINCH_RECEIVED.store(true, Ordering::Relaxed);
}

extern "C" fn sigusr1_handler(_signum: c_int) {
    SIGUSR1_RECEIVED.store(true, Ordering::Relaxed);
}

pub struct Pty {
    master_fd: c_int,
    child_pid: pid_t,
}

impl Pty {
    pub fn new(
        cmd: Option<&[String]>,
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> io::Result<Self> {
        unsafe {
            let mut master: c_int = 0;
            let ws = get_winsize(0)?;
            
            let pid = forkpty(&mut master, std::ptr::null(), std::ptr::null(), &ws);
            
            if pid < 0 {
                return Err(io::Error::last_os_error());
            }
            
            if pid == 0 {
                // Child process
                if let Some(cwd) = cwd {
                    let cwd_cstr = match CString::new(cwd.as_bytes()) {
                        Ok(s) => s,
                        Err(_) => libc::_exit(127),
                    };
                    if libc::chdir(cwd_cstr.as_ptr()) != 0 {
                        libc::_exit(127);
                    }
                }
                
                // Set environment variables
                for (key, value) in env {
                    let key_cstr = match CString::new(key.as_bytes()) {
                        Ok(s) => s,
                        Err(_) => libc::_exit(127),
                    };
                    let value_cstr = match CString::new(value.as_bytes()) {
                        Ok(s) => s,
                        Err(_) => libc::_exit(127),
                    };
                    if libc::setenv(key_cstr.as_ptr(), value_cstr.as_ptr(), 1) != 0 {
                        libc::_exit(127);
                    }
                }
                
                // Execute command
                if let Some(cmd_args) = cmd {
                    if cmd_args.is_empty() {
                        libc::_exit(127);
                    }
                    
                    // Convert to CStrings - must live until execvp
                    let cmd_cstr = match CString::new(cmd_args[0].as_bytes()) {
                        Ok(s) => s,
                        Err(_) => libc::_exit(127),
                    };
                    let argv_cstrs: Vec<CString> = cmd_args
                        .iter()
                        .map(|s| CString::new(s.as_bytes()))
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap_or_else(|_| libc::_exit(127));
                    
                    let mut argv: Vec<*const libc::c_char> = argv_cstrs
                        .iter()
                        .map(|s| s.as_ptr())
                        .collect();
                    argv.push(std::ptr::null());
                    
                    libc::execvp(cmd_cstr.as_ptr(), argv.as_ptr());
                    libc::_exit(127);
                } else {
                    // Use $SHELL
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
                    let shell_cstr = match CString::new(shell.as_bytes()) {
                        Ok(s) => s,
                        Err(_) => libc::_exit(127),
                    };
                    libc::execl(
                        shell_cstr.as_ptr(),
                        shell_cstr.as_ptr(),
                        std::ptr::null::<libc::c_char>(),
                    );
                    libc::_exit(127);
                }
            }
            
            // Parent process
            Ok(Pty {
                master_fd: master,
                child_pid: pid,
            })
        }
    }
    
    pub fn master_fd(&self) -> RawFd {
        self.master_fd
    }
    
    pub fn child_pid(&self) -> pid_t {
        self.child_pid
    }
    
    pub fn set_winsize(&self, ws: libc::winsize) -> io::Result<()> {
        unsafe {
            if libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
    
    pub fn wait_nonblocking(&self) -> io::Result<Option<ProcessStatus>> {
        unsafe {
            let mut status: c_int = 0;
            let pid = libc::waitpid(self.child_pid, &mut status, libc::WNOHANG);
            if pid < 0 {
                return Err(io::Error::last_os_error());
            }
            if pid == 0 {
                return Ok(None);
            }
            
            if libc::WIFEXITED(status) {
                Ok(Some(ProcessStatus::Exited(libc::WEXITSTATUS(status))))
            } else if libc::WIFSIGNALED(status) {
                Ok(Some(ProcessStatus::Signaled(libc::WTERMSIG(status))))
            } else {
                Ok(None)
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.master_fd);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProcessStatus {
    Exited(i32),
    Signaled(i32),
}

pub fn get_winsize(fd: RawFd) -> io::Result<libc::winsize> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) < 0 {
            // Not a TTY or other error - return default size
            ws.ws_col = 80;
            ws.ws_row = 24;
        }
        Ok(ws)
    }
}

pub struct TermMode {
    saved_termios: Option<libc::termios>,
    stdin_fd: RawFd,
}

impl TermMode {
    pub fn set_raw(stdin_fd: RawFd) -> io::Result<Self> {
        unsafe {
            if libc::isatty(stdin_fd) == 0 {
                return Ok(TermMode { saved_termios: None, stdin_fd });
            }
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(stdin_fd, &mut termios) < 0 {
                return Err(io::Error::last_os_error());
            }
            let saved = termios;
            
            // Set raw mode
            libc::cfmakeraw(&mut termios);
            if libc::tcsetattr(stdin_fd, libc::TCSANOW, &termios) < 0 {
                return Err(io::Error::last_os_error());
            }
            
            Ok(TermMode {
                saved_termios: Some(saved),
                stdin_fd,
            })
        }
    }
}

impl Drop for TermMode {
    fn drop(&mut self) {
        if let Some(saved) = self.saved_termios {
            unsafe {
                libc::tcsetattr(self.stdin_fd, libc::TCSANOW, &saved);
            }
        }
    }
}

pub fn setup_sigwinch() -> io::Result<()> {
    unsafe {
        let handler_ptr = sigwinch_handler as usize;
        let old_handler = libc::signal(libc::SIGWINCH, handler_ptr);
        if old_handler == libc::SIG_ERR {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn check_sigwinch() -> bool {
    SIGWINCH_RECEIVED.swap(false, Ordering::Relaxed)
}

pub fn setup_sigusr1() -> io::Result<()> {
    unsafe {
        let handler_ptr = sigusr1_handler as usize;
        let old_handler = libc::signal(libc::SIGUSR1, handler_ptr);
        if old_handler == libc::SIG_ERR {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn check_sigusr1() -> bool {
    SIGUSR1_RECEIVED.swap(false, Ordering::Relaxed)
}
