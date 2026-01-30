use std::env;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use common::error::{Error, system_error, io_error};
use common::session::Session;
use crate::terminal::TerminalBuffer;
use crate::platform::*;
use libc;

// partファイル名を生成（common::part_id 使用: 固定長18文字・辞書順＝時系列）
// 形式: part_<ID>_user.txt
fn generate_part_filename() -> String {
    let id = common::part_id::generate_part_id();
    format!("part_{}_user.txt", id)
}

// ログファイルをフラッシュしてpartファイルにリネームし、console.txtをトランケート
// 空のファイルの場合はpartファイルを作成せず、トランケートのみ行う
fn rollover_log_file(
    log_file_path: &Path,
    session_dir: &Path,
) -> Result<(), Error> {
    use std::fs;
    
    // console.txtが存在し、かつ内容がある場合のみpartファイルにリネーム
    if log_file_path.exists() {
        // ファイルサイズを確認
        let metadata = fs::metadata(log_file_path).map_err(|e| {
            io_error(
                &format!("Failed to get metadata for '{}': {}", log_file_path.display(), e),
                74
            )
        })?;
        
        // ファイルが空でない場合のみpartファイルにリネーム
        if metadata.len() > 0 {
            // partファイル名を生成
            let part_filename = generate_part_filename();
            let part_file_path = session_dir.join(&part_filename);
            
            // console.txtをpartファイルにリネーム（ファイルは既に閉じられている前提）
            fs::rename(log_file_path, &part_file_path).map_err(|e| {
                io_error(
                    &format!("Failed to rename log file to '{}': {}", part_file_path.display(), e),
                    74
                )
            })?;
        }
    }
    
    // console.txtを新しく作成（トランケート）
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_file_path)
        .map_err(|e| {
            io_error(
                &format!("Failed to create new log file '{}': {}", log_file_path.display(), e),
                74
            )
        })?;
    
    Ok(())
}

/// シェル起動コマンドを構築する
///
/// - 環境変数SHELLで指定されたシェルを基本とする
/// - bashの場合、かつ `$AISH_HOME/config/aishrc` が存在する場合は
///   `bash --rcfile <aishrc>` で起動し、aishrcを読み込ませる
fn build_shell_command(shell_path: &str, aish_home: &Path) -> Vec<String> {
    let shell = shell_path.to_string();
    let shell_name = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(shell_path);

    // `$AISH_HOME/config/aishrc` を探す
    let aishrc_path: PathBuf = aish_home.join("config").join("aishrc");

    if shell_name == "bash" && aishrc_path.is_file() {
        vec![
            shell,
            "--rcfile".to_string(),
            aishrc_path.to_string_lossy().to_string(),
        ]
    } else {
        vec![shell]
    }
}

pub fn run_shell(session: &Session) -> Result<i32, Error> {
    // ログファイルのパスを決定（セッションディレクトリ内に配置、プレーンテキスト）
    let log_file_path = session.session_dir().join("console.txt");
    
    // ログファイルを開く（追記モード）
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&log_file_path)
        .map_err(|e| {
            io_error(
                &format!("Failed to open log file '{}': {}", log_file_path.display(), e),
                74
            )
        })?;
    
    // シグナルハンドラを設定
    setup_sigwinch().map_err(|e| {
        system_error(&format!("Failed to setup SIGWINCH: {}", e))
    })?;
    
    setup_sigusr1().map_err(|e| {
        system_error(&format!("Failed to setup SIGUSR1: {}", e))
    })?;
    
    setup_sigusr2().map_err(|e| {
        system_error(&format!("Failed to setup SIGUSR2: {}", e))
    })?;
    
    // aishのプロセスIDを取得
    let aish_pid = std::process::id();
    
    // AISH_PIDファイルに書き込む
    let pid_file_path = session.session_dir().join("AISH_PID");
    std::fs::write(&pid_file_path, aish_pid.to_string())
        .map_err(|e| {
            io_error(
                &format!("Failed to write AISH_PID file '{}': {}", pid_file_path.display(), e),
                74
            )
        })?;
    
    // 環境変数を準備
    let mut env_vars = Vec::new();
    env_vars.push(("AISH_SESSION".to_string(), session.session_dir().to_string_lossy().to_string()));
    env_vars.push(("AISH_HOME".to_string(), session.aish_home().to_string_lossy().to_string()));
    env_vars.push(("AISH_PID".to_string(), aish_pid.to_string()));
    
    // シェルコマンドを準備（bashの場合はaishrcを読み込む）
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let cmd = build_shell_command(&shell, session.aish_home());
    
    // 現在の作業ディレクトリを取得
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "/".to_string());
    
    // PTYを作成
    let pty = Pty::new(
        Some(&cmd),
        Some(&cwd),
        &env_vars,
    ).map_err(|e| {
        system_error(&format!("Failed to create PTY: {}", e))
    })?;
    
    let master_fd = pty.master_fd();
    let _child_pid = pty.child_pid();
    
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
        .map_err(|e| {
            io_error(&format!("Failed to set raw mode: {}", e), 74)
        })?;
    
    // ターミナルバッファを初期化
    let mut terminal_buffer = TerminalBuffer::new();
    
    // メインループ用のバッファ
    const MAX_CHUNK: usize = 32768;
    let mut stdin_buf = vec![0u8; MAX_CHUNK];
    let mut pty_buf = vec![0u8; MAX_CHUNK];
    let mut stdin_eof = false;
    
    loop {
        // SIGWINCHをチェック
        if check_sigwinch() {
            if let Ok(ws) = get_winsize(stdin_fd) {
                let _ = pty.set_winsize(ws);
            }
        }
        
        // ロール/トランケートは常に直列化: 同一イテレーション内で SIGUSR1(roll) → SIGUSR2(truncate) の順で処理し、同時実行しない。
        // SIGUSR1をチェック（ログファイルをフラッシュしてpartファイルにリネーム）
        if check_sigusr1() {
            // 現在のバッファの内容をログファイルに書き込む
            let output = terminal_buffer.output();
            if !output.is_empty() {
                let _ = log_file.write_all(output.as_bytes());
                let _ = log_file.flush();
            }
            
            // バッファをクリア（次回のフラッシュ時に以前の内容が含まれないようにする）
            terminal_buffer.clear();
            
            // ログファイルを閉じる（リネームするために必要）
            drop(log_file);
            
            // ログファイルをフラッシュしてpartファイルにリネーム（失敗時はエラーを返し成功扱いにしない）
            rollover_log_file(&log_file_path, session.session_dir())?;
            
            // ログファイルを再オープン（追記モード）
            log_file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(&log_file_path)
                .map_err(|e| {
                    io_error(
                        &format!("Failed to reopen log file '{}': {}", log_file_path.display(), e),
                        74
                    )
                })?;
        }
        
        // SIGUSR2をチェック（直列化のため roll の後に処理。バッファとconsole.txtをトランケート、partファイルは作成しない）
        // aiコマンドがレスポンスを_assistant.txtとして保存した後に呼ばれ、
        // 同じ内容が_user.txtとして重複保存されることを防ぐ
        if check_sigusr2() {
            // バッファをクリア
            terminal_buffer.clear();
            
            // ログファイルを閉じてトランケート
            drop(log_file);
            
            // console.txtをトランケート（空にする）
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&log_file_path)
                .map_err(|e| {
                    io_error(
                        &format!("Failed to truncate log file '{}': {}", log_file_path.display(), e),
                        74
                    )
                })?;
            
            // ログファイルを再オープン（追記モード）
            log_file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(&log_file_path)
                .map_err(|e| {
                    io_error(
                        &format!("Failed to reopen log file '{}': {}", log_file_path.display(), e),
                        74
                    )
                })?;
        }
        
        // 子プロセスの終了をチェック
        match pty.wait_nonblocking() {
            Ok(Some(status)) => {
                // 最終的なバッファの内容をログファイルに書き込む
                let output = terminal_buffer.output();
                if !output.is_empty() {
                    let _ = log_file.write_all(output.as_bytes());
                    let _ = log_file.write_all(b"\n");
                    let _ = log_file.flush();
                }
                // AISH_PIDファイルを削除
                let _ = std::fs::remove_file(&pid_file_path);
                return Ok(match status {
                    ProcessStatus::Exited(code) => code,
                    ProcessStatus::Signaled(sig) => 128 + sig,
                });
            }
            Ok(None) => {}
            Err(e) => {
                // AISH_PIDファイルを削除
                let _ = std::fs::remove_file(&pid_file_path);
                return Err(io_error(&format!("waitpid failed: {}", e), 74));
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
            return Err(io_error(&format!("poll failed: {}", err), 74));
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
                    // 子プロセスの終了を待つ
                    let mut wait_count = 0;
                    while wait_count < 20 {
                        if let Ok(Some(status)) = pty.wait_nonblocking() {
                            // 最終的なバッファの内容をログファイルに書き込む
                            let output = terminal_buffer.output();
                            if !output.is_empty() {
                                let _ = log_file.write_all(output.as_bytes());
                                let _ = log_file.write_all(b"\n");
                                let _ = log_file.flush();
                            }
                            // AISH_PIDファイルを削除
                            let _ = std::fs::remove_file(&pid_file_path);
                            return Ok(match status {
                                ProcessStatus::Exited(code) => code,
                                ProcessStatus::Signaled(sig) => 128 + sig,
                            });
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        wait_count += 1;
                    }
                    // PTYが消失したが終了ステータスを取得できない場合
                    // 最終的なバッファの内容をログファイルに書き込む
                    let output = terminal_buffer.output();
                    if !output.is_empty() {
                        let _ = log_file.write_all(output.as_bytes());
                        let _ = log_file.write_all(b"\n");
                        let _ = log_file.flush();
                    }
                    // AISH_PIDファイルを削除
                    let _ = std::fs::remove_file(&pid_file_path);
                    return Ok(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_shell_detection() {
        // 環境変数SHELLの確認は統合テストで行う
        // ここでは基本的な構造のテストのみ
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

        let cmd = build_shell_command("/bin/bash", &temp_dir);
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

        let cmd = build_shell_command("/bin/bash", &temp_dir);
        assert_eq!(cmd, vec!["/bin/bash".to_string()]);

        let cmd_sh = build_shell_command("/bin/sh", &temp_dir);
        assert_eq!(cmd_sh, vec!["/bin/sh".to_string()]);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_generate_part_filename_format() {
        let name = generate_part_filename();
        assert!(name.starts_with("part_"));
        assert!(name.ends_with("_user.txt"));

        let prefix = "part_";
        let suffix = "_user.txt";
        let core = &name[prefix.len()..name.len() - suffix.len()];

        // コア部分は8文字固定（base62）、辞書順＝時系列
        assert_eq!(core.len(), 8);
        assert!(core.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
