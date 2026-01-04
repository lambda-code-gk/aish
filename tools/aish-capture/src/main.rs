mod logfmt;
mod platform;
mod util;

#[cfg(debug_assertions)]
use util::debug_log;

use libc;
use logfmt::*;
use platform::*;
use std::io::{self, Seek, SeekFrom, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process;

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err((msg, code)) => {
            #[cfg(debug_assertions)]
            debug_log::debug_log("aish-capture", &format!("ERROR: {} (exit code: {})", msg, code));
            eprintln!("aish-capture: {}", msg);
            code
        }
    };
    process::exit(exit_code);
}

fn run() -> Result<i32, (String, i32)> {
    #[cfg(debug_assertions)]
    {
        if let Some(log_file) = debug_log::init_debug_log() {
            debug_log::debug_log("aish-capture", &format!("Starting aish-capture, debug log: {}", log_file));
        }
    }
    
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
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", &format!("ERROR: Option {} requires an argument", args[i-1]));
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
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", "ERROR: Option --max-chunk requires an argument");
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.max_chunk = args[i].parse()
                    .map_err(|_e| {
                        #[cfg(debug_assertions)]
                        debug_log::debug_log("aish-capture", &format!("ERROR: Invalid max-chunk value: {} ({})", args[i], _e));
                        ("Invalid max-chunk value".to_string(), 64)
                    })?;
                i += 1;
            }
            "--cwd" => {
                i += 1;
                if i >= args.len() {
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", "ERROR: Option --cwd requires an argument");
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.cwd = Some(args[i].clone());
                i += 1;
            }
            "--env" => {
                i += 1;
                if i >= args.len() {
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", "ERROR: Option --env requires an argument");
                    return Err(("Option requires an argument".to_string(), 64));
                }
                let env_str = &args[i];
                if let Some(eq_pos) = env_str.find('=') {
                    let key = env_str[..eq_pos].to_string();
                    let value = env_str[eq_pos + 1..].to_string();
                    config.env.push((key, value));
                } else {
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", &format!("ERROR: Invalid env format: {}", env_str));
                    return Err((format!("Invalid env format: {}", env_str), 64));
                }
                i += 1;
            }
            "--input-fifo" => {
                i += 1;
                if i >= args.len() {
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", "ERROR: Option --input-fifo requires an argument");
                    return Err(("Option requires an argument".to_string(), 64));
                }
                config.input_fifo = Some(args[i].clone());
                i += 1;
            }
            "--" => {
                i += 1;
                cmd_start = Some(i);
                break;
            }
            _ if args[i].starts_with('-') => {
                #[cfg(debug_assertions)]
                debug_log::debug_log("aish-capture", &format!("ERROR: Unknown option: {}", args[i]));
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
    
    #[cfg(debug_assertions)]
    debug_log::debug_log("aish-capture", &format!("Output file: {}", config.output));
    
    execute(config, cmd)
}

struct Config {
    output: String,
    append: bool,
    no_stdin: bool,
    max_chunk: usize,
    cwd: Option<String>,
    env: Vec<(String, String)>,
    input_fifo: Option<String>,
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
            input_fifo: None,
        }
    }
}

// Ensure log file position is correct (handle external truncation)
fn ensure_log_file_position(log_file: &mut std::fs::File) {
    if let (Ok(current_pos), Ok(metadata)) = (log_file.seek(SeekFrom::Current(0)), log_file.metadata()) {
        let file_size = metadata.len();
        if current_pos > file_size {
            // File was truncated externally, reset position to end of file
            let _ = log_file.seek(SeekFrom::End(0));
        }
    }
}

fn execute(config: Config, cmd: Option<Vec<String>>) -> Result<i32, (String, i32)> {
    #[cfg(debug_assertions)]
    debug_log::debug_log("aish-capture", "Opening output file");
    
    // Open output file
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(config.append)
        .truncate(!config.append)
        .open(&config.output)
        .map_err(|e| {
            #[cfg(debug_assertions)]
            debug_log::debug_log("aish-capture", &format!("ERROR: Failed to open output file: {} - {}", config.output, e));
            (format!("Failed to open output file: {}", e), 74)
        })?;
    
    #[cfg(debug_assertions)]
    debug_log::debug_log("aish-capture", "Setting up signal handlers");
    
    // Setup SIGWINCH handler
    setup_sigwinch().map_err(|e| {
        #[cfg(debug_assertions)]
        debug_log::debug_log("aish-capture", &format!("ERROR: Failed to setup SIGWINCH: {}", e));
        (format!("Failed to setup SIGWINCH: {}", e), 1)
    })?;
    
    // Setup SIGUSR1 handler
    setup_sigusr1().map_err(|e| {
        #[cfg(debug_assertions)]
        debug_log::debug_log("aish-capture", &format!("ERROR: Failed to setup SIGUSR1: {}", e));
        (format!("Failed to setup SIGUSR1: {}", e), 1)
    })?;
    
    #[cfg(debug_assertions)]
    {
        let cmd_str = cmd.as_ref()
            .map(|v| v.join(" "))
            .unwrap_or_else(|| "shell".to_string());
        debug_log::debug_log("aish-capture", &format!("Creating PTY for command: {}", cmd_str));
    }
    
    // Create PTY
    let pty = Pty::new(
        cmd.as_ref().map(|v| v.as_slice()),
        config.cwd.as_deref(),
        &config.env,
    ).map_err(|e| {
        #[cfg(debug_assertions)]
        {
            let cmd_str = cmd.as_ref()
                .map(|v| v.join(" "))
                .unwrap_or_else(|| "shell".to_string());
            debug_log::debug_log("aish-capture", &format!("ERROR: Failed to create PTY for command: {} - {}", cmd_str, e));
        }
        (format!("Failed to create PTY: {}", e), 70)
    })?;
    
    #[cfg(debug_assertions)]
    debug_log::debug_log("aish-capture", "PTY created successfully");
    
    let master_fd = pty.master_fd();
    let child_pid = pty.child_pid();
    
    // Set master_fd to non-blocking
    unsafe {
        let flags = libc::fcntl(master_fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
    
    // Set terminal to raw mode
    let stdin_fd = io::stdin().as_raw_fd();
    let _term_mode = TermMode::set_raw(stdin_fd)
        .map_err(|e| {
            #[cfg(debug_assertions)]
            debug_log::debug_log("aish-capture", &format!("ERROR: Failed to set raw mode: {}", e));
            (format!("Failed to set raw mode: {}", e), 74)
        })?;
    
    // Get initial window size
    let initial_ws = get_winsize(stdin_fd)
        .map_err(|e| {
            #[cfg(debug_assertions)]
            debug_log::debug_log("aish-capture", &format!("ERROR: Failed to get window size: {}", e));
            (format!("Failed to get window size: {}", e), 74)
        })?;
    
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
    ensure_log_file_position(&mut log_file);
    write_start(
        &mut log_file,
        initial_ws.ws_col,
        initial_ws.ws_row,
        &cmd_argv,
        cwd_str,
        child_pid,
    ).map_err(|e| {
        #[cfg(debug_assertions)]
        debug_log::debug_log("aish-capture", &format!("ERROR: Failed to write start event: {}", e));
        (format!("Failed to write start event: {}", e), 74)
    })?;
    
    #[cfg(debug_assertions)]
    debug_log::debug_log("aish-capture", "Start event written, entering main loop");
    
    // Open FIFO if specified (non-blocking mode)
    let fifo_file: Option<std::fs::File> = if let Some(ref fifo_path) = config.input_fifo {
        unsafe {
            use std::ffi::CString;
            let path_cstr = CString::new(fifo_path.as_str())
                .map_err(|e| {
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", &format!("ERROR: Invalid FIFO path: {} - {}", fifo_path, e));
                    (format!("Invalid FIFO path: {}", e), 74)
                })?;
            let fd = libc::open(
                path_cstr.as_ptr(),
                libc::O_RDWR | libc::O_NONBLOCK,
            );
            if fd < 0 {
                let err = io::Error::last_os_error();
                #[cfg(debug_assertions)]
                debug_log::debug_log("aish-capture", &format!("ERROR: Failed to open FIFO: {} - {}", fifo_path, err));
                return Err((format!("Failed to open FIFO: {}", err), 74));
            }
            Some(std::fs::File::from_raw_fd(fd))
        }
    } else {
        None
    };
    let fifo_fd = fifo_file.as_ref().map(|f| f.as_raw_fd());
    let mut fifo_eof = false;
    
    // Main poll loop
    let mut stdin_buf = vec![0u8; config.max_chunk];
    let mut pty_buf = vec![0u8; config.max_chunk];
    let mut fifo_buf = vec![0u8; config.max_chunk];
    let mut stdin_eof = false;
    let mut stdin_text_buffer = logfmt::TextBuffer::new();
    let mut fifo_text_buffer = logfmt::TextBuffer::new();
    let mut stdout_text_buffer = logfmt::TextBuffer::new();
    
    loop {
        // Check for SIGWINCH
        if check_sigwinch() {
            if let Ok(ws) = get_winsize(stdin_fd) {
                let _ = pty.set_winsize(ws);
                ensure_log_file_position(&mut log_file);
                let _ = write_resize(&mut log_file, ws.ws_col, ws.ws_row);
            }
        }
        
        // Check for SIGUSR1 (flush log file and write buffered data)
        if check_sigusr1() {
            // Flush stdin buffer if there's data
            if let Some(data) = stdin_text_buffer.flush() {
                if !config.no_stdin {
                    ensure_log_file_position(&mut log_file);
                    let _ = write_stdin(&mut log_file, &data);
                }
            }
            // Flush stdout buffer if there's data
            if let Some(data) = stdout_text_buffer.flush() {
                ensure_log_file_position(&mut log_file);
                let _ = write_stdout(&mut log_file, &data);
            }
            // Flush log file to disk
            let _ = log_file.flush();
        }
        
        // Check if child process has exited
        match pty.wait_nonblocking() {
            Ok(Some(status)) => {
                #[cfg(debug_assertions)]
                {
                    let status_str = match status {
                        ProcessStatus::Exited(code) => format!("exited with code {}", code),
                        ProcessStatus::Signaled(sig) => format!("killed by signal {}", sig),
                    };
                    debug_log::debug_log("aish-capture", &format!("Child process exited: {}", status_str));
                }
                return Ok(finalize_and_get_exit_code(
                    status,
                    &mut log_file,
                    &mut stdin_text_buffer,
                    &mut fifo_text_buffer,
                    &mut stdout_text_buffer,
                    &config,
                ));
            }
            Ok(None) => {}
            Err(e) => {
                #[cfg(debug_assertions)]
                debug_log::debug_log("aish-capture", &format!("ERROR: waitpid failed: {}", e));
                return Err((format!("waitpid failed: {}", e), 74));
            }
        }
        
        // Poll setup
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
        
        let mut fifo_idx = None;
        if let Some(fd) = fifo_fd {
            if !fifo_eof {
                fifo_idx = Some(pollfds.len());
                pollfds.push(libc::pollfd { fd, events: libc::POLLIN, revents: 0 });
            }
        }
        
        let timeout_ms = 50;
        let n = unsafe {
            libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::c_ulong, timeout_ms)
        };
        
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted { continue; }
            #[cfg(debug_assertions)]
            debug_log::debug_log("aish-capture", &format!("ERROR: poll failed: {}", err));
            return Err((format!("poll failed: {}", err), 74));
        }
        
        if n == 0 { continue; }

        // Read from stdin
        if let Some(idx) = stdin_idx {
            if (pollfds[idx].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0 {
                let n = unsafe { libc::read(stdin_fd, stdin_buf.as_mut_ptr() as *mut libc::c_void, config.max_chunk) };
                if n > 0 {
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", &format!("Read {} bytes from stdin", n));
                    let chunk = &stdin_buf[..n as usize];
                    let _ = unsafe { libc::write(master_fd, chunk.as_ptr() as *const libc::c_void, n as usize) };
                    if !config.no_stdin {
                        for data in stdin_text_buffer.append(chunk) {
                            ensure_log_file_position(&mut log_file);
                            let _ = write_stdin(&mut log_file, &data);
                        }
                    }
                } else if n == 0 {
                    #[cfg(debug_assertions)]
                    debug_log::debug_log("aish-capture", "Stdin EOF");
                    if let Some(data) = stdin_text_buffer.flush() {
                        if !config.no_stdin {
                            ensure_log_file_position(&mut log_file);
                            let _ = write_stdin(&mut log_file, &data);
                        }
                    }
                    stdin_eof = true;
                }
            }
        }
        
        // Read from FIFO
        if let Some(idx) = fifo_idx {
            if let Some(fd) = fifo_fd {
                if (pollfds[idx].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0 {
                    let n = unsafe { libc::read(fd, fifo_buf.as_mut_ptr() as *mut libc::c_void, config.max_chunk) };
                    if n > 0 {
                        let chunk = &fifo_buf[..n as usize];
                        let _ = unsafe { libc::write(master_fd, chunk.as_ptr() as *const libc::c_void, n as usize) };
                        for data in fifo_text_buffer.append(chunk) {
                            ensure_log_file_position(&mut log_file);
                            let _ = write_stdin(&mut log_file, &data);
                        }
                    } else if n == 0 {
                        // EOF on FIFO (should not happen with O_RDWR usually, but handle it)
                        if let Some(data) = fifo_text_buffer.flush() {
                            ensure_log_file_position(&mut log_file);
                            let _ = write_stdin(&mut log_file, &data);
                        }
                        fifo_eof = true;
                    }
                }
            }
        }
        
        // Read from PTY
        if (pollfds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0 {
            let n = unsafe { libc::read(master_fd, pty_buf.as_mut_ptr() as *mut libc::c_void, config.max_chunk) };
            if n > 0 {
                #[cfg(debug_assertions)]
                debug_log::debug_log("aish-capture", &format!("Read {} bytes from PTY", n));
                let chunk = &pty_buf[..n as usize];
                let _ = io::stdout().write_all(chunk);
                let _ = io::stdout().flush();
                for data in stdout_text_buffer.append(chunk) {
                    ensure_log_file_position(&mut log_file);
                    let _ = write_stdout(&mut log_file, &data);
                }
            } else if n <= 0 {
                // EOF or error on PTY - check if child still alive
                let err = if n < 0 { Some(io::Error::last_os_error()) } else { None };
                let is_real_end = match err {
                    Some(ref e) => {
                        let errno = e.raw_os_error().unwrap_or(0);
                        e.kind() != io::ErrorKind::Interrupted && errno != libc::EAGAIN && errno != libc::EWOULDBLOCK
                    }
                    None => true,
                };
                
                if is_real_end {
                    // Try to wait for child one last time
                    let mut wait_count = 0;
                    while wait_count < 20 {
                        if let Ok(Some(status)) = pty.wait_nonblocking() {
                            return Ok(finalize_and_get_exit_code(
                                status,
                                &mut log_file,
                                &mut stdin_text_buffer,
                                &mut fifo_text_buffer,
                                &mut stdout_text_buffer,
                                &config,
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        wait_count += 1;
                    }
                    // If we get here, the PTY is gone but we can't get the exit status.
                    return Ok(0); 
                }
            }
        }
    }
}

fn finalize_and_get_exit_code(
    status: ProcessStatus,
    log_file: &mut std::fs::File,
    stdin_text_buffer: &mut TextBuffer,
    fifo_text_buffer: &mut TextBuffer,
    stdout_text_buffer: &mut TextBuffer,
    config: &Config,
) -> i32 {
    if let Some(data) = stdin_text_buffer.flush() {
        if !config.no_stdin {
            ensure_log_file_position(log_file);
            let _ = write_stdin(log_file, &data);
        }
    }
    if let Some(data) = fifo_text_buffer.flush() {
        ensure_log_file_position(log_file);
        let _ = write_stdin(log_file, &data);
    }
    if let Some(data) = stdout_text_buffer.flush() {
        ensure_log_file_position(log_file);
        let _ = write_stdout(log_file, &data);
    }
    
    match status {
        ProcessStatus::Exited(code) => {
            ensure_log_file_position(log_file);
            let _ = write_exit_code(log_file, code);
            code
        }
        ProcessStatus::Signaled(sig) => {
            ensure_log_file_position(log_file);
            let _ = write_exit_signal(log_file, sig);
            128 + sig
        }
    }
}
