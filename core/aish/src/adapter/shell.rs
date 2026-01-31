use std::env;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use common::adapter::{FileSystem, PtyProcessStatus, PtySpawn, Signal, Winsize};
use common::error::Error;
use common::part_id::IdGenerator;
use crate::adapter::platform::{get_winsize, TermMode};
use crate::adapter::terminal::TerminalBuffer;
use libc;

fn part_filename_from_id(id: &common::domain::PartId) -> String {
    format!("part_{}_user.txt", id)
}

/// ログファイルをフラッシュしてpartファイルにリネームし、console.txtをトランケート（アダプター経由）
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

/// シェル起動コマンドを構築（aishrc の有無は fs で判定）
fn build_shell_command<F: FileSystem + ?Sized>(
    shell_path: &str,
    aish_home: &Path,
    fs: &F,
) -> Vec<String> {
    let shell = shell_path.to_string();
    let shell_name = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(shell_path);

    let aishrc_path: PathBuf = aish_home.join("config").join("aishrc");
    let use_aishrc = shell_name == "bash"
        && fs.exists(&aishrc_path)
        && fs.metadata(&aishrc_path).map(|m| m.is_file()).unwrap_or(false);

    if use_aishrc {
        vec![
            shell,
            "--rcfile".to_string(),
            aishrc_path.to_string_lossy().to_string(),
        ]
    } else {
        vec![shell]
    }
}

fn libc_winsize_to_common(ws: libc::winsize) -> Winsize {
    Winsize {
        ws_row: ws.ws_row,
        ws_col: ws.ws_col,
        ws_xpixel: ws.ws_xpixel,
        ws_ypixel: ws.ws_ypixel,
    }
}

/// アダプター経由でシェルを起動（Unix 専用）
#[cfg(unix)]
pub fn run_shell(
    session_dir: &Path,
    home_dir: &Path,
    fs: &dyn FileSystem,
    id_gen: &dyn IdGenerator,
    signal: &dyn Signal,
    pty_spawn: &dyn PtySpawn,
) -> Result<i32, Error> {
    let log_file_path = session_dir.join("console.txt");

    let mut log_file = fs.open_append(&log_file_path)?;

    signal.setup_sigwinch()?;
    signal.setup_sigusr1()?;
    signal.setup_sigusr2()?;

    let aish_pid = std::process::id();
    let pid_file_path = session_dir.join("AISH_PID");
    fs.write(&pid_file_path, &aish_pid.to_string())?;

    let mut env_vars = Vec::new();
    env_vars.push(("AISH_SESSION".to_string(), session_dir.to_string_lossy().to_string()));
    env_vars.push(("AISH_HOME".to_string(), home_dir.to_string_lossy().to_string()));
    env_vars.push(("AISH_PID".to_string(), aish_pid.to_string()));

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let cmd = build_shell_command(&shell, home_dir, fs);

    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "/".to_string());

    let pty = pty_spawn.spawn(Some(&cmd), Some(&cwd), &env_vars)?;
    let master_fd = pty.master_fd();
    
    // master_fdを非ブロッキングモードに設定
    unsafe {
        let flags = libc::fcntl(master_fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
    
    // ターミナルをrawモードに設定
    let stdin_fd = io::stdin().as_raw_fd();
    let _term_mode = TermMode::set_raw(stdin_fd)
        .map_err(|e| Error::io_msg(format!("Failed to set raw mode: {}", e)))?;
    
    // ターミナルバッファを初期化
    let mut terminal_buffer = TerminalBuffer::new();
    
    // メインループ用のバッファ
    const MAX_CHUNK: usize = 32768;
    let mut stdin_buf = vec![0u8; MAX_CHUNK];
    let mut pty_buf = vec![0u8; MAX_CHUNK];
    let mut stdin_eof = false;
    
    loop {
        if signal.check_sigwinch() {
            if let Ok(ws) = get_winsize(stdin_fd) {
                let common_ws = libc_winsize_to_common(ws);
                let _ = pty.set_winsize(&common_ws);
            }
        }

        if signal.check_sigusr1() {
            let output = terminal_buffer.output();
            if !output.is_empty() {
                let _ = log_file.write_all(output.as_bytes());
                let _ = log_file.flush();
            }
            terminal_buffer.clear();
            drop(log_file);
            rollover_log_file(&log_file_path, session_dir, fs, id_gen)?;
            log_file = fs.open_append(&log_file_path)?;
        }

        if signal.check_sigusr2() {
            terminal_buffer.clear();
            drop(log_file);
            fs.truncate_file(&log_file_path)?;
            log_file = fs.open_append(&log_file_path)?;
        }

        match pty.wait_nonblocking() {
            Ok(Some(status)) => {
                let output = terminal_buffer.output();
                if !output.is_empty() {
                    let _ = log_file.write_all(output.as_bytes());
                    let _ = log_file.write_all(b"\n");
                    let _ = log_file.flush();
                }
                let _ = fs.remove_file(&pid_file_path);
                return Ok(match status {
                    PtyProcessStatus::Exited(code) => code,
                    PtyProcessStatus::Signaled(sig) => 128 + sig,
                });
            }
            Ok(None) => {}
            Err(e) => {
                let _ = fs.remove_file(&pid_file_path);
                return Err(e);
            }
        }
        
        // pollのセットアップ
        let mut pollfds = vec![
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        
        let mut stdin_idx = None;
        if !stdin_eof {
            stdin_idx = Some(pollfds.len());
            pollfds.push(libc::pollfd { fd: stdin_fd, events: libc::POLLIN, revents: 0 });
        }
        
        let timeout_ms = 50;
        let n = unsafe {
            libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::c_ulong, timeout_ms)
        };
        
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(Error::io_msg(format!("poll failed: {}", err)));
        }
        
        if n == 0 {
            // タイムアウト
            continue;
        }
        
        // 標準入力から読み取り
        if let Some(idx) = stdin_idx {
            if (pollfds[idx].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0 {
                let n = unsafe { libc::read(stdin_fd, stdin_buf.as_mut_ptr() as *mut libc::c_void, MAX_CHUNK) };
                if n > 0 {
                    let chunk = &stdin_buf[..n as usize];
                    let _ = unsafe { libc::write(master_fd, chunk.as_ptr() as *const libc::c_void, n as usize) };
                    // 標準入力はログに記録しない（プレーンテキストログでは不要）
                } else if n == 0 {
                    // EOF
                    stdin_eof = true;
                }
            }
        }
        
        // PTYから読み取り
        if (pollfds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0 {
            let n = unsafe { libc::read(master_fd, pty_buf.as_mut_ptr() as *mut libc::c_void, MAX_CHUNK) };
            if n > 0 {
                let chunk = &pty_buf[..n as usize];
                // 標準出力に表示
                let _ = io::stdout().write_all(chunk);
                let _ = io::stdout().flush();
                // ターミナルバッファで処理（ANSIエスケープシーケンスを処理してプレーンテキストに変換）
                terminal_buffer.process_data(chunk);
            } else if n <= 0 {
                // EOFまたはエラー - 子プロセスがまだ生きているかチェック
                let err = if n < 0 { Some(io::Error::last_os_error()) } else { None };
                let is_real_end = match err {
                    Some(ref e) => {
                        let errno = e.raw_os_error().unwrap_or(0);
                        e.kind() != io::ErrorKind::Interrupted && errno != libc::EAGAIN && errno != libc::EWOULDBLOCK
                    }
                    None => true,
                };
                
                if is_real_end {
                    let mut wait_count = 0;
                    while wait_count < 20 {
                        if let Ok(Some(status)) = pty.wait_nonblocking() {
                            let output = terminal_buffer.output();
                            if !output.is_empty() {
                                let _ = log_file.write_all(output.as_bytes());
                                let _ = log_file.write_all(b"\n");
                                let _ = log_file.flush();
                            }
                            let _ = fs.remove_file(&pid_file_path);
                            return Ok(match status {
                                PtyProcessStatus::Exited(code) => code,
                                PtyProcessStatus::Signaled(sig) => 128 + sig,
                            });
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        wait_count += 1;
                    }
                    let output = terminal_buffer.output();
                    if !output.is_empty() {
                        let _ = log_file.write_all(output.as_bytes());
                        let _ = log_file.write_all(b"\n");
                        let _ = log_file.flush();
                    }
                    let _ = fs.remove_file(&pid_file_path);
                    return Ok(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;
    use common::domain::PartId;
    use std::fs;

    #[test]
    fn test_shell_detection() {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        assert!(!shell.is_empty());
    }

    #[test]
    fn test_build_shell_command_uses_aishrc_for_bash() {
        let temp_dir = std::env::temp_dir().join("aish_test_aishrc_bash");
        let config_dir = temp_dir.join("config");
        let aishrc = config_dir.join("aishrc");

        if aishrc.exists() {
            let _ = fs::remove_file(&aishrc);
        }
        if config_dir.exists() {
            let _ = fs::remove_dir_all(&config_dir);
        }
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }

        fs::create_dir_all(&config_dir).unwrap();
        fs::write(&aishrc, b"# test aishrc").unwrap();

        let fs_adapter = StdFileSystem;
        let cmd = build_shell_command("/bin/bash", &temp_dir, &fs_adapter);
        assert_eq!(cmd.len(), 3);
        assert_eq!(cmd[0], "/bin/bash");
        assert_eq!(cmd[1], "--rcfile");
        assert_eq!(Path::new(&cmd[2]), aishrc.as_path());

        let _ = fs::remove_file(&aishrc);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_build_shell_command_without_aishrc_falls_back() {
        let temp_dir = std::env::temp_dir().join("aish_test_no_aishrc");
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        fs::create_dir_all(&temp_dir).unwrap();

        let fs_adapter = StdFileSystem;
        let cmd = build_shell_command("/bin/bash", &temp_dir, &fs_adapter);
        assert_eq!(cmd, vec!["/bin/bash".to_string()]);

        let cmd_sh = build_shell_command("/bin/sh", &temp_dir, &fs_adapter);
        assert_eq!(cmd_sh, vec!["/bin/sh".to_string()]);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_part_filename_format() {
        let id = PartId::generate();
        let name = part_filename_from_id(&id);
        assert!(name.starts_with("part_"));
        assert!(name.ends_with("_user.txt"));

        let prefix = "part_";
        let suffix = "_user.txt";
        let core = &name[prefix.len()..name.len() - suffix.len()];
        assert_eq!(core.len(), 8);
        assert!(core.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
