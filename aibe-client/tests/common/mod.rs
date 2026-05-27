#![cfg(unix)]

//! mock `aibe` バイナリ起動（`aibe-client` は `aibe` クレートに依存しない）。

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aibe_client::ping;
use tempfile::TempDir;

/// `cargo test` 実行時の `aibe` バイナリ（`aibe` への path 依存なし）。
pub fn aibe_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_aibe") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return path;
        }
    }
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../target/{profile}/aibe"))
}

/// 一時設定で mock provider の `aibe -f` を起動し、socket が応答するまで待つ。
pub struct MockAibeDaemon {
    _dir: TempDir,
    child: Child,
    pub socket_path: PathBuf,
}

impl MockAibeDaemon {
    pub fn start() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let config_path = dir.path().join("aibe.toml");
        fs::write(&config_path, "[llm]\nprovider = \"mock\"\n").expect("write aibe config");
        let _ = fs::remove_file(&socket_path);

        let bin = aibe_binary();
        assert!(
            bin.is_file(),
            "aibe binary not found at {} (run `cargo build -p aibe` first)",
            bin.display()
        );

        let mut child = Command::new(&bin)
            .arg("-f")
            .env("AIBE_CONFIG", &config_path)
            .env("AIBE_SOCKET_PATH", &socket_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn aibe");

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if ping(&socket_path) {
                return Self {
                    _dir: dir,
                    child,
                    socket_path,
                };
            }
            if child.try_wait().expect("try_wait").is_some() {
                panic!("aibe exited before socket was ready");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        panic!(
            "timed out waiting for aibe socket at {}",
            socket_path.display()
        );
    }
}

impl Drop for MockAibeDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.socket_path);
    }
}
