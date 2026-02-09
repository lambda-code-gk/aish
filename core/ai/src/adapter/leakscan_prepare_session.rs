//! leakscan を用いたセッション準備（part の監査・reviewed 作成・退避移動）
//!
//! セッション dir 内の part_* を leakscan し、ヒット時はユーザーに問い合わせ、
//! 通過分は reviewed_<id>_... にコピー、元 part は leakscan_evacuated/ に移動する。

use crate::ports::outbound::{InterruptChecker, PrepareSessionForSensitiveCheck};
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::FileSystem;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

const EVACUATED_DIR: &str = "leakscan_evacuated";

/// leakscan のユーザー選択（y/n/a/m）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SensitiveChoice {
    Allow,
    Deny,
    Mask,
}

/// part ファイル名から id と role を抽出する。
/// 例: part_ABC12xyz_user.txt -> (ABC12xyz, "user"), part_ABC12xyz_assistant.txt -> (ABC12xyz, "assistant")
fn parse_part_filename(name: &str) -> Option<(String, &'static str)> {
    if !name.starts_with("part_") {
        return None;
    }
    let rest = &name[5..]; // after "part_"
    if rest.ends_with("_user.txt") {
        let id = rest[..rest.len() - 9].to_string(); // remove _user.txt
        if !id.is_empty() {
            return Some((id, "user"));
        }
    } else if rest.ends_with("_assistant.txt") {
        let id = rest[..rest.len() - 13].to_string(); // remove _assistant.txt
        if !id.is_empty() {
            return Some((id, "assistant"));
        }
    }
    None
}

/// reviewed ファイル名を生成
fn reviewed_filename(id: &str, role: &str) -> String {
    format!("reviewed_{}_{}.txt", id, role)
}

pub struct LeakscanPrepareSession {
    fs: Arc<dyn FileSystem>,
    leakscan_binary: PathBuf,
    rules_path: PathBuf,
    interrupt_checker: Option<Arc<dyn InterruptChecker>>,
    /// true のときヒット時に対話せず常に Deny（CI 等向け）
    non_interactive: bool,
}

impl LeakscanPrepareSession {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        leakscan_binary: PathBuf,
        rules_path: PathBuf,
        interrupt_checker: Option<Arc<dyn InterruptChecker>>,
        non_interactive: bool,
    ) -> Self {
        Self {
            fs,
            leakscan_binary,
            rules_path,
            interrupt_checker,
            non_interactive,
        }
    }

    /// 内容を leakscan に渡し、ヒットしたら true（stdout にマッチ行が出る）。-v で理由付き出力を取得
    fn leakscan_check(&self, content: &str) -> Result<(bool, String), Error> {
        let rules = self.rules_path.to_string_lossy();
        let mut cmd = Command::new(&self.leakscan_binary);
        cmd.args(["-v", rules.as_ref()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| Error::io_msg(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| Error::io_msg(e.to_string()))?;
        }
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .map_err(|e| Error::io_msg(e.to_string()))?;
        let status = child.wait().map_err(|e| Error::io_msg(e.to_string()))?;
        let hit = status.success() && !stdout.trim().is_empty();
        Ok((hit, stdout))
    }

    /// 内容を leakscan --mask に渡し、マスク済み文字列を返す
    fn leakscan_mask(&self, content: &str) -> Result<String, Error> {
        let rules = self.rules_path.to_string_lossy();
        let mut cmd = Command::new(&self.leakscan_binary);
        cmd.args(["--mask", rules.as_ref()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| Error::io_msg(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| Error::io_msg(e.to_string()))?;
        }
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .map_err(|e| Error::io_msg(e.to_string()))?;
        let _ = child.wait();
        Ok(stdout)
    }

    /// ヒット時に対話で y/n/a/m を聞く
    fn prompt_sensitive_choice(&self, verbose_output: &str) -> Result<SensitiveChoice, Error> {
        eprintln!("\x1b[1;33mSECURITY: Sensitive content matched\x1b[0m");
        eprintln!("----------------------------------------");
        eprint!("{}", verbose_output);
        eprintln!("----------------------------------------");
        eprint!("Send to LLM? [y]es / [n]o (deny) / [m]ask: ");
        std::io::stderr().flush().map_err(|e| Error::io_msg(e.to_string()))?;

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let choice = if std::io::stdin().read_line(&mut line).is_ok() {
                match line.trim().to_lowercase().as_str() {
                    "y" | "yes" | "" => SensitiveChoice::Allow,
                    "n" | "no" => SensitiveChoice::Deny,
                    "m" | "mask" => SensitiveChoice::Mask,
                    _ => SensitiveChoice::Deny,
                }
            } else {
                SensitiveChoice::Deny
            };
            let _ = tx.send(choice);
        });

        let timeout = std::time::Duration::from_millis(100);
        loop {
            match rx.recv_timeout(timeout) {
                Ok(choice) => return Ok(choice),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if self
                        .interrupt_checker
                        .as_ref()
                        .map_or(false, |c| c.is_interrupted())
                    {
                        return Err(Error::system(
                            "Interrupted by user (Ctrl+C) during sensitive check prompt.",
                        ));
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Ok(SensitiveChoice::Deny);
                }
            }
        }
    }

    /// 1 つの part を処理: leakscan → 問い合わせ → reviewed 作成 & 退避
    fn process_one_part(
        &self,
        session_dir: &Path,
        part_path: &Path,
        content: String,
    ) -> Result<(), Error> {
        let name = part_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::io_msg("Invalid part filename"))?;
        let (id, role) = parse_part_filename(name).ok_or_else(|| {
            Error::io_msg(format!("Could not parse part filename: {}", name))
        })?;

        let evacuated_dir = session_dir.join(EVACUATED_DIR);
        let dest_part_in_evacuated = evacuated_dir.join(name);

        let (should_allow, content_for_reviewed) = match self.leakscan_check(&content)? {
            (false, _) => (true, content),
            (true, verbose_output) => {
                let choice = if self.non_interactive {
                    SensitiveChoice::Deny
                } else {
                    self.prompt_sensitive_choice(&verbose_output)?
                };
                match choice {
                    SensitiveChoice::Allow => (true, content),
                    SensitiveChoice::Deny => {
                        self.fs.rename(part_path, &dest_part_in_evacuated)?;
                        return Ok(());
                    }
                    SensitiveChoice::Mask => {
                        let masked = self.leakscan_mask(&content)?;
                        (true, masked)
                    }
                }
            }
        };

        if should_allow {
            let reviewed_name = reviewed_filename(&id, role);
            let reviewed_path = session_dir.join(&reviewed_name);
            self.fs.write(&reviewed_path, &content_for_reviewed)?;
            self.fs.rename(part_path, &dest_part_in_evacuated)?;
        }
        Ok(())
    }
}

impl PrepareSessionForSensitiveCheck for LeakscanPrepareSession {
    fn prepare(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let dir = session_dir.as_ref();
        if !self.fs.exists(dir) {
            return Ok(());
        }
        if self
            .fs
            .metadata(dir)
            .map(|m| !m.is_dir())
            .unwrap_or(true)
        {
            return Ok(());
        }

        let mut part_files: Vec<PathBuf> = self
            .fs
            .read_dir(dir)?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |s| s.starts_with("part_"))
                    && self.fs.metadata(path).map(|m| m.is_file()).unwrap_or(false)
            })
            .collect();
        part_files.sort();

        if part_files.is_empty() {
            return Ok(());
        }

        let evacuated_dir = dir.join(EVACUATED_DIR);
        if !self.fs.exists(&evacuated_dir) {
            self.fs.create_dir_all(&evacuated_dir)?;
        }

        for part_path in part_files {
            let content = match self.fs.read_to_string(&part_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to read part '{}': {}",
                        part_path.display(),
                        e
                    );
                    continue;
                }
            };
            if let Err(e) = self.process_one_part(dir, &part_path, content) {
                return Err(e);
            }
        }
        Ok(())
    }
}
