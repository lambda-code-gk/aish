mod logfmt;
mod platform;
mod util;

use libc;
use logfmt::*;
use platform::*;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::process;

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err((msg, code)) => {
            eprintln!("aish-capture: {}", msg);
            code
        }
    };
    process::exit(exit_code);
}

fn run() -> Result<i32, (String, i32)> {
    let args: Vec<String> = std::env::args().collect();
    let mut config = Config::default();
    let mut cmd_start = None;
    
    // Simple argument parsing
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.output = args[i].clone();
                i += 1;
            }
            "--append" => {
                config.append = true;
                i += 1;
            }
            "--no-stdin" => {
                config.no_stdin = true;
                i += 1;
            }
            "--max-chunk" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.max_chunk = args[i].parse()
                    .map_err(|_| ("Invalid max-chunk value".to_string(), 64))?;
                i += 1;
            }
            "--cwd" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.cwd = Some(args[i].clone());
                i += 1;
            }
            "--env" => {
                i += 1;
                if i >= args.len() {
                    return Err(("Option requires an argument".to_string(), 64));
                }
                let env_str = &args[i];
                if let Some(eq_pos) = env_str.find('=') {
                    let key = env_str[..eq_pos].to_string();
                    let value = env_str[eq_pos + 1..].to_string();
                    config.env.push((key, value));
                } else {
                    return Err((format!("Invalid env format: {}", env_str), 64));
                }
                i += 1;
            }
            "--" => {
                i += 1;
                cmd_start = Some(i);
                break;
            }
            _ if args[i].starts_with('-') => {
                return Err((format!("Unknown option: {}", args[i]), 64));
            }
            _ => {
                cmd_start = Some(i);
                break;
            }
        }
    }
    
    let cmd = if let Some(start) = cmd_start {
        if start < args.len() {
            Some(args[start..].to_vec())
        } else {
            None
        }
    } else {
        None
    };
    
    // Default output filename
    if config.output.is_empty() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        config.output = format!("./aish-capture-{}.jsonl", timestamp);
    }
    
    execute(config, cmd)
}

struct Config {
    output: String,
    append: bool,
    no_stdin: bool,
    max_chunk: usize,
    cwd: Option<String>,
    env: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output: String::new(),
            append: false,
            no_stdin: false,
            max_chunk: 32768,
            cwd: None,
            env: Vec::new(),
        }
    }
}

fn execute(config: Config, cmd: Option<Vec<String>>) -> Result<i32, (String, i32)> {
    // Open output file
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(config.append)
        .truncate(!config.append)
        .open(&config.output)
        .map_err(|e| (format!("Failed to open output file: {}", e), 74))?;
    
    // Setup SIGWINCH handler
    setup_sigwinch().map_err(|e| (format!("Failed to setup SIGWINCH: {}", e), 1))?;
    
    // Setup SIGUSR1 handler
    setup_sigusr1().map_err(|e| (format!("Failed to setup SIGUSR1: {}", e), 1))?;
    
    // Create PTY
    let pty = Pty::new(
        cmd.as_ref().map(|v| v.as_slice()),
        config.cwd.as_deref(),
        &config.env,
    ).map_err(|e| (format!("Failed to create PTY: {}", e), 70))?;
    
    let master_fd = pty.master_fd();
    let child_pid = pty.child_pid();
    
    // Set terminal to raw mode
    let stdin_fd = io::stdin().as_raw_fd();
    let _term_mode = TermMode::set_raw(stdin_fd)
        .map_err(|e| (format!("Failed to set raw mode: {}", e), 74))?;
    
    // Get initial window size
    let initial_ws = get_winsize(stdin_fd)
        .map_err(|e| (format!("Failed to get window size: {}", e), 74))?;
    
    // Write start event
    let cwd_string = config.cwd.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/".to_string())
    });
    let cwd_str = cwd_string.as_str();
    let cmd_argv = cmd.unwrap_or_else(|| {
        vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
    });
    write_start(
        &mut log_file,
        initial_ws.ws_col,
        initial_ws.ws_row,
        &cmd_argv,
        cwd_str,
        child_pid,
    ).map_err(|e| (format!("Failed to write start event: {}", e), 74))?;
    
    // Main poll loop
    let mut stdin_buf = vec![0u8; config.max_chunk];
    let mut pty_buf = vec![0u8; config.max_chunk];
    let mut stdin_eof = false;
    let mut stdin_text_buffer = logfmt::TextBuffer::new();
    let mut stdout_text_buffer = logfmt::TextBuffer::new();
    
    loop {
        // Check for SIGWINCH
        if check_sigwinch() {
            if let Ok(ws) = get_winsize(stdin_fd) {
                let _ = pty.set_winsize(ws);
                let _ = write_resize(&mut log_file, ws.ws_col, ws.ws_row);
            }
        }
        
        // Check for SIGUSR1 (flush log file)
        if check_sigusr1() {
            let _ = log_file.flush();
        }
        
        // Check if child process has exited
        match pty.wait_nonblocking() {
            Ok(Some(status)) => {
                // Flush remaining buffered data
                if let Some(data) = stdin_text_buffer.flush() {
                    if !config.no_stdin {
                        let _ = write_stdin(&mut log_file, &data);
                    }
                }
                if let Some(data) = stdout_text_buffer.flush() {
                    let _ = write_stdout(&mut log_file, &data);
                }
                
                let exit_code = match status {
                    ProcessStatus::Exited(code) => {
                        write_exit_code(&mut log_file, code)
                            .map_err(|e| (format!("Failed to write exit event: {}", e), 74))?;
                        code
                    }
                    ProcessStatus::Signaled(sig) => {
                        write_exit_signal(&mut log_file, sig)
                            .map_err(|e| (format!("Failed to write exit event: {}", e), 74))?;
                        128 + sig
                    }
                };
                return Ok(exit_code);
            }
            Ok(None) => {
                // Process still running, continue
            }
            Err(e) => {
                return Err((format!("waitpid failed: {}", e), 74));
            }
        }
        
        // Poll stdin and pty
        let mut pollfds = vec![
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        
        if !stdin_eof {
            pollfds.insert(0, libc::pollfd {
                fd: stdin_fd,
                events: libc::POLLIN,
                revents: 0,
            });
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
            return Err((format!("poll failed: {}", err), 74));
        }
        
        // Read from stdin and write to pty
        if !stdin_eof {
            let stdin_idx = 0;
            if (pollfds[stdin_idx].revents & libc::POLLIN) != 0 {
                unsafe {
                    let n = libc::read(stdin_fd, stdin_buf.as_mut_ptr() as *mut libc::c_void, config.max_chunk);
                    if n > 0 {
                        let chunk = &stdin_buf[..n as usize];
                        // Write to pty
                        let written = libc::write(master_fd, chunk.as_ptr() as *const libc::c_void, n as usize);
                        if written >= 0 {
                            // Buffer and log stdin event (if enabled)
                            if !config.no_stdin {
                                let lines = stdin_text_buffer.append(chunk);
                                for data in lines {
                                    let _ = write_stdin(&mut log_file, &data);
                                }
                            }
                        }
                    } else if n == 0 {
                        // EOF on stdin - flush remaining buffered data
                        if let Some(data) = stdin_text_buffer.flush() {
                            if !config.no_stdin {
                                let _ = write_stdin(&mut log_file, &data);
                            }
                        }
                        stdin_eof = true;
                    } else {
                        let err = io::Error::last_os_error();
                        if err.kind() == io::ErrorKind::Interrupted {
                            // Continue on interrupt
                        } else {
                            // Other error, treat as EOF
                            stdin_eof = true;
                        }
                    }
                }
            }
        }
        
        // Read from pty and write to stdout
        let pty_idx = if stdin_eof { 0 } else { 1 };
        if (pollfds[pty_idx].revents & libc::POLLIN) != 0 {
            unsafe {
                let n = libc::read(master_fd, pty_buf.as_mut_ptr() as *mut libc::c_void, config.max_chunk);
                if n > 0 {
                    let chunk = &pty_buf[..n as usize];
                    // Write to stdout
                    let _ = io::stdout().write_all(chunk);
                    let _ = io::stdout().flush();
                    // Buffer and log stdout event
                    let lines = stdout_text_buffer.append(chunk);
                    for data in lines {
                        let _ = write_stdout(&mut log_file, &data);
                    }
                } else if n == 0 {
                    // EOF on pty - flush remaining buffered data
                    if let Some(data) = stdout_text_buffer.flush() {
                        let _ = write_stdout(&mut log_file, &data);
                    }
                    // EOF on pty - child may have exited, will be caught by wait_nonblocking
                } else {
                    let err = io::Error::last_os_error();
                    if err.kind() != io::ErrorKind::Interrupted {
                        // Ignore other errors
                    }
                }
            }
        }
    }
}

