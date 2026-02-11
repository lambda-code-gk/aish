//! leakscan を用いたセッション準備（part の監査・reviewed 作成・退避移動）
//!
//! セッション dir 内の part_* を leakscan し、ヒット時はユーザーに問い合わせ、
//! 通過分は reviewed_<id>_... にコピー、元 part は leakscan_evacuated/ に移動する。

use crate::adapter::session_manifest;
use crate::ports::outbound::{CompactionStrategy, InterruptChecker, PrepareSessionForSensitiveCheck};
use crate::domain::{
    hash64, ManifestDecision, ManifestRecordV1, ManifestRole, MessageRecordV1,
};
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::now_iso8601;
use common::ports::outbound::FileSystem;
use common::safe_session_path::REVIEWED_DIR;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

const EVACUATED_DIR: &str = "leakscan_evacuated";

/// Deny 時に reviewed に書き込む固定文（機微情報を含めない）
pub(crate) const DENY_PLACEHOLDER_CONTENT: &str = "[REDACTED] content denied by user.\n";

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

fn role_from_str(role: &str) -> Result<ManifestRole, Error> {
    match role {
        "user" => Ok(ManifestRole::User),
        "assistant" => Ok(ManifestRole::Assistant),
        _ => Err(Error::io_msg(format!("Unknown role: {}", role))),
    }
}

pub struct LeakscanPrepareSession {
    fs: Arc<dyn FileSystem>,
    leakscan_binary: PathBuf,
    rules_path: PathBuf,
    interrupt_checker: Option<Arc<dyn InterruptChecker>>,
    /// true のときヒット時に対話せず常に Deny（CI 等向け）
    non_interactive: bool,
    /// compaction 実装（None のときは compaction しない）
    compaction: Option<Arc<dyn CompactionStrategy>>,
}

impl LeakscanPrepareSession {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        leakscan_binary: PathBuf,
        rules_path: PathBuf,
        interrupt_checker: Option<Arc<dyn InterruptChecker>>,
        non_interactive: bool,
        compaction: Option<Arc<dyn CompactionStrategy>>,
    ) -> Self {
        Self {
            fs,
            leakscan_binary,
            rules_path,
            interrupt_checker,
            non_interactive,
            compaction,
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
            if let Err(e) = stdin.write_all(content.as_bytes()) {
                if e.kind() != ErrorKind::BrokenPipe {
                    return Err(Error::io_msg(e.to_string()));
                }
            }
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
            if let Err(e) = stdin.write_all(content.as_bytes()) {
                if e.kind() != ErrorKind::BrokenPipe {
                    return Err(Error::io_msg(e.to_string()));
                }
            }
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

        let (decision, content_for_reviewed) = match self.leakscan_check(&content)? {
            (false, _) => (ManifestDecision::Allow, content),
            (true, verbose_output) => {
                let choice = if self.non_interactive {
                    SensitiveChoice::Deny
                } else {
                    self.prompt_sensitive_choice(&verbose_output)?
                };
                match choice {
                    SensitiveChoice::Allow => (ManifestDecision::Allow, content),
                    SensitiveChoice::Deny => {
                        (ManifestDecision::Deny, DENY_PLACEHOLDER_CONTENT.to_string())
                    }
                    SensitiveChoice::Mask => {
                        let masked = self.leakscan_mask(&content)?;
                        (ManifestDecision::Mask, masked)
                    }
                }
            }
        };

        let reviewed_basename = reviewed_filename(&id, role);
        let reviewed_subdir = session_dir.join(REVIEWED_DIR);
        if !self.fs.exists(&reviewed_subdir) {
            self.fs.create_dir_all(&reviewed_subdir)?;
        }
        let reviewed_path = reviewed_subdir.join(&reviewed_basename);
        self.fs.write(&reviewed_path, &content_for_reviewed)?;
        self.fs.rename(part_path, &dest_part_in_evacuated)?;

        let reviewed_path_in_manifest = format!("{}/{}", REVIEWED_DIR, reviewed_basename);
        let rec = ManifestRecordV1::Message(MessageRecordV1 {
            v: 1,
            ts: now_iso8601(),
            id,
            role: role_from_str(role)?,
            part_path: name.to_string(),
            reviewed_path: reviewed_path_in_manifest,
            decision,
            bytes: content_for_reviewed.len() as u64,
            hash64: hash64(&content_for_reviewed),
        });
        session_manifest::append(self.fs.as_ref(), session_dir, &rec)?;
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

        let evacuated_dir = dir.join(EVACUATED_DIR);
        if !part_files.is_empty() && !self.fs.exists(&evacuated_dir) {
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
        if let Some(ref c) = self.compaction {
            let records = session_manifest::load_all(self.fs.as_ref(), dir)?;
            c.maybe_compact(self.fs.as_ref(), dir, &records)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{parse_lines, ManifestDecision};
    use common::adapter::StdFileSystem;
    use std::io::Write;

    /// -v を付けると stdout に HIT を出して exit 0 するダミー leakscan
    fn write_dummy_leakscan_script(path: &Path) -> std::io::Result<()> {
        let mut f = std::fs::File::create(path)?;
        #[cfg(unix)]
        {
            f.write_all(b"#!/bin/sh\ncase \"$1\" in -v) echo HIT;; *) ;; esac\n")?;
        }
        #[cfg(not(unix))]
        {
            f.write_all(b"echo HIT\n")?;
        }
        f.flush()?;
        f.sync_all()?;
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms)?;
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_deny_creates_reviewed_placeholder_and_evacuates() {
        let temp = tempfile::tempdir().unwrap();
        let session_dir = temp.path().to_path_buf();
        let script_path = temp.path().join("leakscan.sh");
        let rules_path = temp.path().join("rules.json");
        std::fs::write(&rules_path, "{}").unwrap();
        write_dummy_leakscan_script(&script_path).unwrap();

        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let prep = LeakscanPrepareSession::new(
            fs.clone(),
            script_path,
            rules_path,
            None,
            true,
            None,
        );

        let part_content = "secret stuff";
        let part_path = session_dir.join("part_ABC12_user.txt");
        std::fs::write(&part_path, part_content).unwrap();

        let session_dir_ref = common::domain::SessionDir::new(session_dir.clone());
        prep.prepare(&session_dir_ref).unwrap();

        let reviewed_path = session_dir.join(REVIEWED_DIR).join("reviewed_ABC12_user.txt");
        assert!(fs.exists(&reviewed_path), "reviewed file should exist");
        let reviewed_content = std::fs::read_to_string(&reviewed_path).unwrap();
        assert_eq!(reviewed_content, DENY_PLACEHOLDER_CONTENT);

        let evacuated_path = session_dir.join(EVACUATED_DIR).join("part_ABC12_user.txt");
        assert!(fs.exists(&evacuated_path), "part should be in evacuated");

        let manifest_path = session_dir.join("manifest.jsonl");
        assert!(fs.exists(&manifest_path), "manifest should exist");
        let manifest_body = std::fs::read_to_string(manifest_path).unwrap();
        let records = parse_lines(&manifest_body);
        assert!(!records.is_empty(), "manifest should contain at least one line");
        let first = records.first().and_then(|r| r.message()).unwrap();
        assert_eq!(first.id, "ABC12");
        assert_eq!(first.role, ManifestRole::User);
        assert_eq!(first.decision, ManifestDecision::Deny);
        assert_eq!(first.reviewed_path, "reviewed/reviewed_ABC12_user.txt");
    }
}
