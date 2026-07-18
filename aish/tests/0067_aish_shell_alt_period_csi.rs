#![cfg(unix)]
//! 0067: `aish shell` で Alt+. 後も CSI 矢印が line editor に解釈されること。
//!
//! 再現条件は replay DEBUG trap が bind -x 中に動くこと（Issue #11 / PR #13）。

use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PROMPT: &str = "__AISH_0067_SHELL__ ";
const TIMEOUT: Duration = Duration::from_secs(10);

fn set_winsize(fd: i32, row: u16, col: u16) {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    size.ws_row = row;
    size.ws_col = col;
    assert_eq!(
        unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) },
        0,
        "TIOCSWINSZ"
    );
}

fn require_aish() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aish"))
}

fn which_ai() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_ai") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return p;
        }
    }
    // aish の integration test は ai bin をリンクしないため、同 target dir を探す。
    let sibling = require_aish().with_file_name("ai");
    if sibling.is_file() {
        return sibling;
    }
    let candidates = [
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/ai"),
        PathBuf::from("target/debug/ai"),
    ];
    for path in candidates {
        if path.is_file() {
            return path;
        }
    }
    panic!("ai binary not found for PATH; build -p ai first");
}

#[test]
fn aish_shell_alt_period_preserves_csi_navigation() {
    let aish = require_aish();
    let ai = which_ai();
    let home = tempfile::tempdir().expect("home");
    fs::write(home.path().join(".bashrc"), format!("PS1='{PROMPT}'\n")).expect("bashrc");

    let mut master = -1;
    let mut slave = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0,
        "openpty"
    );
    set_winsize(master, 40, 120);
    set_winsize(slave, 40, 120);
    let slave_file = unsafe { fs::File::from_raw_fd(slave) };
    let path = format!(
        "{}:{}:{}",
        aish.parent().expect("aish dir").display(),
        ai.parent().expect("ai dir").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(&aish);
    command
        .arg("shell")
        .env("HOME", home.path())
        .env("PATH", &path)
        .env("TERM", "xterm")
        .env("SHELL", "/bin/bash")
        .stdin(Stdio::from(slave_file.try_clone().expect("clone stdin")))
        .stdout(Stdio::from(slave_file.try_clone().expect("clone stdout")))
        .stderr(Stdio::from(slave_file));
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("spawn aish shell");
    let mut master_file = unsafe { fs::File::from_raw_fd(master) };
    let flags = unsafe { libc::fcntl(master_file.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe {
            libc::fcntl(
                master_file.as_raw_fd(),
                libc::F_SETFL,
                flags | libc::O_NONBLOCK,
            )
        },
        0
    );

    let mut transcript = Vec::new();
    expect(&mut master_file, &mut transcript, PROMPT, "initial prompt");
    transcript.clear();

    master_file.write_all(b"aabb").expect("type");
    master_file.flush().ok();
    thread::sleep(Duration::from_millis(150));
    master_file.write_all(b"\x1b.").expect("Alt+.");
    master_file.flush().ok();
    // bind -x の再描画にも PS1 が含まれるため、ここで PROMPT 待ちすると CSI 前に成功扱いになる。
    // 再描画が落ち着くまで読み捨ててから CSI を送る。
    drain_for(
        &mut master_file,
        &mut transcript,
        Duration::from_millis(900),
    );
    transcript.clear();

    master_file
        .write_all(b"\x1b[D\x1b[D\x1b[DX\n")
        .expect("CSI+marker");
    master_file.flush().ok();
    expect(&mut master_file, &mut transcript, PROMPT, "after edit");

    let text = String::from_utf8_lossy(&transcript);
    assert!(
        !text.contains("^[[D") && !text.contains("^[[C"),
        "CSI must not appear as caret escapes after Alt+.: {text}"
    );
    assert!(
        text.contains("aXabb"),
        "left arrows after Alt+. must edit the line: {text}"
    );

    let _ = master_file.write_all(b"exit\n");
    let _ = master_file.flush();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn drain_for(master: &mut fs::File, transcript: &mut Vec<u8>, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        let mut buf = [0u8; 1024];
        match master.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                transcript.extend_from_slice(&buf[..n]);
                if transcript.len() > 64 * 1024 {
                    transcript.drain(..32 * 1024);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(15));
            }
            Err(err) => panic!("drain read: {err}"),
        }
    }
}

fn expect(master: &mut fs::File, transcript: &mut Vec<u8>, needle: &str, label: &str) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if transcript
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
        {
            return;
        }
        let mut buf = [0u8; 1024];
        match master.read(&mut buf) {
            Ok(0) => panic!(
                "{label}: PTY closed; transcript={}",
                String::from_utf8_lossy(transcript)
            ),
            Ok(n) => {
                transcript.extend_from_slice(&buf[..n]);
                if transcript.len() > 64 * 1024 {
                    transcript.drain(..32 * 1024);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    panic!(
                        "{label}: timeout waiting for {needle:?}; transcript={}",
                        String::from_utf8_lossy(transcript)
                    );
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(err) => panic!("{label}: read: {err}"),
        }
    }
}
