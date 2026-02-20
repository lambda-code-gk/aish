use std::env;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use common::domain::event::{Event, RunId, SessionId};
use common::domain::{PendingInput, PolicyStatus};
use common::error::Error;
use common::event_hub::build_event_hub;
use common::ports::outbound::{EnvResolver, FileSystem, PtyProcessStatus, PtySpawn, Signal, Winsize};
use common::part_id::IdGenerator;
use crate::adapter::console_handler::ConsoleLogHandler;
use crate::adapter::platform::{get_winsize, TermMode};
use crate::adapter::prompt_ready_detector::PromptReadyDetector;
use crate::adapter::terminal::TerminalBuffer;
use crate::domain::SessionEvent;
use crate::ports::outbound::ShellRunner;
use libc;

const PENDING_INPUT_FILENAME: &str = "pending_input.json";
const PROMPT_SUGGESTION_FILENAME: &str = "prompt_suggestion.txt";
const PENDING_MAX_LEN: usize = 4096;

/// ShellRunner の標準実装（run_shell をラップ）
#[cfg(unix)]
pub struct StdShellRunner {
    env_resolver: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
    id_gen: Arc<dyn IdGenerator>,
    signal: Arc<dyn Signal>,
    pty_spawn: Arc<dyn PtySpawn>,
}

#[cfg(unix)]
impl StdShellRunner {
    pub fn new(
        env_resolver: Arc<dyn EnvResolver>,
        fs: Arc<dyn FileSystem>,
        id_gen: Arc<dyn IdGenerator>,
        signal: Arc<dyn Signal>,
        pty_spawn: Arc<dyn PtySpawn>,
    ) -> Self {
        Self {
            env_resolver,
            fs,
            id_gen,
            signal,
            pty_spawn,
        }
    }
}

#[cfg(unix)]
impl ShellRunner for StdShellRunner {
    fn run(&self, session_dir: &Path, home_dir: &Path) -> Result<i32, Error> {
        run_shell(
            session_dir,
            home_dir,
            self.env_resolver.clone(),
            self.fs.clone(),
            self.id_gen.as_ref(),
            self.signal.as_ref(),
            self.pty_spawn.as_ref(),
        )
    }
}

/// AISH_HOME 未設定時用の aishrc 候補パスを返す（$XDG_CONFIG_HOME/aish/aishrc, ~/.config/aish/aishrc, ~/.aishrc）
fn default_aishrc_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let config_base = env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var("HOME")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        });
    if let Some(mut p) = config_base {
        p.push("aish");
        p.push("aishrc");
        candidates.push(p);
    }
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            candidates.push(PathBuf::from(home).join(".aishrc"));
        }
    }
    candidates
}

/// シェル起動コマンドを構築（aishrc の有無は fs で判定）
///
/// - AISH_HOME が設定されている場合のみ、$AISH_HOME/config/aishrc を最優先候補にする
/// - AISH_HOME 未設定時は XDG / HOME ベースのフォールバック候補のみを使う
fn build_shell_command<F: FileSystem + ?Sized>(shell_path: &str, fs: &F) -> Vec<String> {
    let shell = shell_path.to_string();
    let shell_name = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(shell_path);

    let mut candidates: Vec<PathBuf> = Vec::new();

    // AISH_HOME がある場合のみ $AISH_HOME/config/aishrc を優先候補に追加
    if let Ok(aish_home) = env::var("AISH_HOME") {
        if !aish_home.is_empty() {
            candidates.push(PathBuf::from(aish_home).join("config").join("aishrc"));
        }
    }

    candidates.extend(default_aishrc_candidates());

    let aishrc_path = (shell_name == "bash").then(|| {
        candidates.into_iter().find(|p| {
            fs.exists(p) && fs.metadata(p).map(|m| m.is_file()).unwrap_or(false)
        })
    }).flatten();

    if let Some(path) = aishrc_path {
        vec![
            shell,
            "--rcfile".to_string(),
            path.to_string_lossy().to_string(),
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

/// pending_input.json から PendingInput を読み出す。無い・不正なら None。agent_state.json は読まない。
fn try_load_pending_input(session_dir: &Path, fs: &dyn FileSystem) -> Result<Option<PendingInput>, Error> {
    let path = session_dir.join(PENDING_INPUT_FILENAME);
    if !fs.exists(&path) {
        return Ok(None);
    }
    let s = fs.read_to_string(&path)?;
    let p: PendingInput = serde_json::from_str(&s).map_err(|e| Error::json(e.to_string()))?;
    Ok(Some(p))
}

/// pending_input.json を削除する。agent_state.json は触らない。
fn clear_pending_input(session_dir: &Path, fs: &dyn FileSystem) -> Result<(), Error> {
    let path = session_dir.join(PENDING_INPUT_FILENAME);
    if fs.exists(&path) {
        fs.remove_file(&path)?;
    }
    Ok(())
}

/// prompt_suggestion.txt を削除する（Alt+S 注入後にプロンプトを通常表示に戻すため）。
fn clear_prompt_suggestion(session_dir: &Path, fs: &dyn FileSystem) -> Result<(), Error> {
    let path = session_dir.join(PROMPT_SUGGESTION_FILENAME);
    if fs.exists(&path) {
        fs.remove_file(&path)?;
    }
    Ok(())
}

/// 注入用文字列をサニタイズ（ESC・危険な制御文字除去、最大長）。改行・CR・タブは許可。
fn sanitize_for_inject(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(PENDING_MAX_LEN));
    let mut count = 0usize;
    for ch in s.chars() {
        if (ch as u32) < 0x20 && ch != '\t' && ch != '\n' && ch != '\r' {
            continue;
        }
        if ch == '\x7f' {
            continue;
        }
        out.push(ch);
        count += 1;
        if count >= PENDING_MAX_LEN {
            out.push('…');
            break;
        }
    }
    out
}

/// PTY master に pending をそのまま注入（Ctrl-U で行クリア → Bracketed Paste）。blocked は廃止し常にそのまま注入する。
fn inject_pending_to_pty(master_fd: libc::c_int, pending: &PendingInput) -> Result<(), Error> {
    let text = sanitize_for_inject(&pending.text);
    if text.is_empty() {
        return Ok(());
    }
    const CTRL_U: u8 = 0x15;
    const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
    const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
    unsafe {
        let _ = libc::write(master_fd, [CTRL_U].as_ptr() as *const libc::c_void, 1);
        let _ = libc::write(master_fd, BRACKETED_PASTE_START.as_ptr() as *const libc::c_void, BRACKETED_PASTE_START.len());
        let _ = libc::write(master_fd, text.as_ptr() as *const libc::c_void, text.len());
        let _ = libc::write(master_fd, BRACKETED_PASTE_END.as_ptr() as *const libc::c_void, BRACKETED_PASTE_END.len());
    }
    Ok(())
}

fn policy_status_str(p: &PolicyStatus) -> &'static str {
    match p {
        PolicyStatus::Allowed => "allowed",
        PolicyStatus::NeedsWarning { .. } => "warn",
        PolicyStatus::Blocked { .. } => "blocked",
    }
}

/// アダプター経由でシェルを起動（Unix 専用）
#[cfg(unix)]
pub fn run_shell(
    session_dir: &Path,
    _home_dir: &Path,
    env: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
    id_gen: &dyn IdGenerator,
    signal: &dyn Signal,
    pty_spawn: &dyn PtySpawn,
) -> Result<i32, Error> {
    let session_dir_value = common::domain::SessionDir::new(session_dir.to_path_buf());
    let event_hub = build_event_hub(Some(&session_dir_value), env, fs.clone(), false);
    let fs_ref = fs.as_ref();
    let session_id = SessionId::new(session_dir.display().to_string());
    let run_id = RunId::new("inject");

    let log_file_path = session_dir.join("console.txt");
    let mute_flag_path = session_dir.join("console.muted");

    let mut log_file = fs_ref.open_append(&log_file_path)?;

    signal.setup_sigwinch()?;
    signal.setup_sigusr1()?;
    signal.setup_sigusr2()?;

    let aish_pid = std::process::id();
    let pid_file_path = session_dir.join("AISH_PID");
    fs_ref.write(&pid_file_path, &aish_pid.to_string())?;

    let mut env_vars = Vec::new();
    env_vars.push(("AISH_SESSION".to_string(), session_dir.to_string_lossy().to_string()));
    env_vars.push(("AISH_PID".to_string(), aish_pid.to_string()));

    // 親プロセス環境に既に AISH_HOME がある場合のみ、それを子プロセスに伝播する
    if let Ok(aish_home) = env::var("AISH_HOME") {
        if !aish_home.is_empty() {
            env_vars.push(("AISH_HOME".to_string(), aish_home));
        }
    }

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let cmd = build_shell_command(&shell, fs_ref);

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

    // イベントハンドラ（flush / rollover / truncate を集約）
    let handler = ConsoleLogHandler::new(&log_file_path, session_dir, fs_ref, id_gen);

    // PromptReady マーカー検知（次のプロンプトで pending を注入するため）
    let mut prompt_ready_detector = PromptReadyDetector::new();
    
    // メインループ用のバッファ（Alt+S = \x1b s 検出用に 1 バイト保留）
    const MAX_CHUNK: usize = 32768;
    let mut stdin_buf = vec![0u8; MAX_CHUNK];
    let mut pty_buf = vec![0u8; MAX_CHUNK];
    let mut stdin_eof = false;
    let mut stdin_esc_pending: Option<u8> = None;

    loop {
        // シグナルをイベントに変換してハンドラに渡す
        let events: Vec<SessionEvent> = [
            (signal.check_sigwinch(), SessionEvent::SigWinch),
            (signal.check_sigusr1(), SessionEvent::SigUsr1),
            (signal.check_sigusr2(), SessionEvent::SigUsr2),
        ]
        .into_iter()
        .filter_map(|(triggered, ev)| if triggered { Some(ev) } else { None })
        .collect();

        for event in events {
            match event {
                SessionEvent::SigWinch => {
                    if let Ok(ws) = get_winsize(stdin_fd) {
                        let common_ws = libc_winsize_to_common(ws);
                        let _ = pty.set_winsize(&common_ws);
                    }
                }
                SessionEvent::SigUsr1 | SessionEvent::SigUsr2 => {
                    let output = terminal_buffer.output();
                    terminal_buffer.clear();
                    log_file = handler.handle(event, &output, log_file)?;
                }
            }
        }

        match pty.wait_nonblocking() {
            Ok(Some(status)) => {
                let output = terminal_buffer.output();
                if !output.is_empty() && !fs_ref.exists(&mute_flag_path) {
                    let _ = log_file.write_all(output.as_bytes());
                    let _ = log_file.write_all(b"\n");
                    let _ = log_file.flush();
                }
                let _ = fs_ref.remove_file(&pid_file_path);
                return Ok(match status {
                    PtyProcessStatus::Exited(code) => code,
                    PtyProcessStatus::Signaled(sig) => 128 + sig,
                });
            }
            Ok(None) => {}
            Err(e) => {
                let _ = fs_ref.remove_file(&pid_file_path);
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
        
        // 標準入力から読み取り（Alt+S = \x1b s のときは注入し、キーはシェルに渡さない）
        if let Some(idx) = stdin_idx {
            if (pollfds[idx].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0 {
                let n = unsafe { libc::read(stdin_fd, stdin_buf.as_mut_ptr() as *mut libc::c_void, MAX_CHUNK) };
                if n > 0 {
                    let n = n as usize;
                    let mut chunk = Vec::with_capacity(stdin_esc_pending.as_ref().map(|_| 1).unwrap_or(0) + n);
                    if let Some(b) = stdin_esc_pending.take() {
                        chunk.push(b);
                    }
                    chunk.extend_from_slice(&stdin_buf[..n]);

                    let mut i = 0;
                    while i + 2 <= chunk.len() && chunk[i] == 0x1b && chunk[i + 1] == b's' {
                        if let Ok(Some(pending)) = try_load_pending_input(session_dir, fs_ref) {
                            let _ = inject_pending_to_pty(master_fd, &pending);
                            event_hub.emit(Event {
                                v: 1,
                                session_id: session_id.clone(),
                                run_id: run_id.clone(),
                                kind: "shell.suggestion.injected".to_string(),
                                payload: serde_json::json!({
                                    "ok": true,
                                    "policy": policy_status_str(&pending.policy),
                                    "bytes": pending.text.len(),
                                }),
                            });
                            let _ = clear_pending_input(session_dir, fs_ref);
                            let _ = clear_prompt_suggestion(session_dir, fs_ref);
                        }
                        i += 2;
                    }
                    let to_forward: &[u8] = if i >= chunk.len() {
                        &[]
                    } else if chunk.len() == i + 1 && chunk[i] == 0x1b {
                        stdin_esc_pending = Some(0x1b);
                        &chunk[..i]
                    } else {
                        &chunk[i..]
                    };
                    if !to_forward.is_empty() {
                        let _ = unsafe {
                            libc::write(master_fd, to_forward.as_ptr() as *const libc::c_void, to_forward.len())
                        };
                    }
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
                // PromptReady マーカー検知 → 注入は行わず、pending があればベルを鳴らす
                if prompt_ready_detector.feed(chunk) {
                    if let Ok(Some(_)) = try_load_pending_input(session_dir, fs_ref) {
                        let _ = io::stdout().write_all(b"\x07");
                        let _ = io::stdout().flush();
                    }
                }
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
                            if !output.is_empty() && !fs_ref.exists(&mute_flag_path) {
                                let _ = log_file.write_all(output.as_bytes());
                                let _ = log_file.write_all(b"\n");
                                let _ = log_file.flush();
                            }
                            let _ = fs_ref.remove_file(&pid_file_path);
                            return Ok(match status {
                                PtyProcessStatus::Exited(code) => code,
                                PtyProcessStatus::Signaled(sig) => 128 + sig,
                            });
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        wait_count += 1;
                    }
                    let output = terminal_buffer.output();
                    if !output.is_empty() && !fs_ref.exists(&mute_flag_path) {
                        let _ = log_file.write_all(output.as_bytes());
                        let _ = log_file.write_all(b"\n");
                        let _ = log_file.flush();
                    }
                    let _ = fs_ref.remove_file(&pid_file_path);
                    return Ok(0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::console_handler::part_filename_from_id;
    use common::adapter::StdFileSystem;
    use common::domain::{PartId, PolicyStatus};
    use std::env;
    use std::fs;
    use std::sync::{Mutex, OnceLock};

    fn aish_home_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_shell_detection() {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        assert!(!shell.is_empty());
    }

    #[test]
    fn test_build_shell_command_uses_aishrc_for_bash() {
        let _guard = aish_home_test_lock().lock().expect("lock poisoned");
        let prev_aish_home = env::var("AISH_HOME").ok();

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

        env::set_var("AISH_HOME", &temp_dir);
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(&aishrc, b"# test aishrc").unwrap();

        let fs_adapter = StdFileSystem;
        let cmd = build_shell_command("/bin/bash", &fs_adapter);
        assert_eq!(cmd.len(), 3);
        assert_eq!(cmd[0], "/bin/bash");
        assert_eq!(cmd[1], "--rcfile");
        assert_eq!(Path::new(&cmd[2]), aishrc.as_path());

        let _ = fs::remove_file(&aishrc);
        let _ = fs::remove_dir_all(&temp_dir);

        if let Some(v) = prev_aish_home {
            env::set_var("AISH_HOME", v);
        } else {
            env::remove_var("AISH_HOME");
        }
    }

    #[test]
    fn test_build_shell_command_without_aishrc_falls_back() {
        let _guard = aish_home_test_lock().lock().expect("lock poisoned");
        let prev_aish_home = env::var("AISH_HOME").ok();
        let prev_xdg_config = env::var("XDG_CONFIG_HOME").ok();
        let prev_home = env::var("HOME").ok();

        let temp_dir = std::env::temp_dir().join("aish_test_no_aishrc");
        if temp_dir.exists() {
            let _ = fs::remove_dir_all(&temp_dir);
        }
        fs::create_dir_all(&temp_dir).unwrap();

        // AISH_HOME / XDG_CONFIG_HOME / HOME をテスト用にアイソレートして、
        // 実環境の aishrc に依存しないようにする
        env::remove_var("AISH_HOME");
        env::set_var("XDG_CONFIG_HOME", temp_dir.join("xdg_config"));
        env::set_var("HOME", temp_dir.join("home"));

        let fs_adapter = StdFileSystem;
        let cmd = build_shell_command("/bin/bash", &fs_adapter);
        assert_eq!(cmd, vec!["/bin/bash".to_string()]);

        let cmd_sh = build_shell_command("/bin/sh", &fs_adapter);
        assert_eq!(cmd_sh, vec!["/bin/sh".to_string()]);

        let _ = fs::remove_dir_all(&temp_dir);

        // 環境変数を元に戻す
        match prev_aish_home {
            Some(v) => env::set_var("AISH_HOME", v),
            None => env::remove_var("AISH_HOME"),
        }
        match prev_xdg_config {
            Some(v) => env::set_var("XDG_CONFIG_HOME", v),
            None => env::remove_var("XDG_CONFIG_HOME"),
        }
        match prev_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
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
        assert!(core.chars().all(|c: char| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_try_load_pending_input_none_when_missing() {
        let tmp = std::env::temp_dir().join("aish_test_pending_missing");
        if tmp.exists() {
            let _ = fs::remove_dir_all(&tmp);
        }
        fs::create_dir_all(&tmp).unwrap();
        let fs = StdFileSystem;
        let got = try_load_pending_input(&tmp, &fs).unwrap();
        assert!(got.is_none());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_try_load_pending_input_some_when_file_exists() {
        let tmp = std::env::temp_dir().join("aish_test_pending_exists");
        if tmp.exists() {
            let _ = fs::remove_dir_all(&tmp);
        }
        fs::create_dir_all(&tmp).unwrap();
        let fs = StdFileSystem;
        let path = tmp.join(PENDING_INPUT_FILENAME);
        let payload = serde_json::json!({
            "text": "git status",
            "policy": "Allowed",
            "created_at_unix_ms": 0,
            "source": "test"
        });
        fs.write(&path, &payload.to_string()).unwrap();
        let got = try_load_pending_input(&tmp, &fs).unwrap();
        assert!(got.is_some());
        let p = got.unwrap();
        assert_eq!(p.text, "git status");
        assert!(matches!(p.policy, PolicyStatus::Allowed));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_clear_pending_input_removes_file() {
        let tmp = std::env::temp_dir().join("aish_test_pending_clear");
        if tmp.exists() {
            let _ = fs::remove_dir_all(&tmp);
        }
        fs::create_dir_all(&tmp).unwrap();
        let fs = StdFileSystem;
        let path = tmp.join(PENDING_INPUT_FILENAME);
        fs.write(&path, r#"{"text":"x","policy":"Allowed","created_at_unix_ms":0,"source":"t"}"#)
            .unwrap();
        assert!(fs.exists(&path));
        clear_pending_input(&tmp, &fs).unwrap();
        assert!(!fs.exists(&path));
        let _ = fs::remove_dir_all(&tmp);
    }
}
