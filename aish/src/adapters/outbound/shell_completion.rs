//! `aish shell` 子プロセス向けの一時 rcfile 生成。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const BASH_SNIPPET: &str = r#"if command -v aish >/dev/null 2>&1; then eval "$(aish complete bash)"; fi
if command -v ai >/dev/null 2>&1; then eval "$(ai complete bash)"; fi
if command -v aibe >/dev/null 2>&1; then eval "$(aibe complete bash)"; fi
"#;

const ZSH_SNIPPET: &str = r#"if command -v aish >/dev/null 2>&1; then eval "$(aish complete zsh)"; fi
if command -v ai >/dev/null 2>&1; then eval "$(ai complete zsh)"; fi
if command -v aibe >/dev/null 2>&1; then eval "$(aibe complete zsh)"; fi
"#;

/// 子シェル種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildShellKind {
    Bash,
    Zsh,
    Other,
}

pub fn detect_child_shell(shell_path: &str) -> ChildShellKind {
    let base = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    match base {
        "bash" | "bash-bin" => ChildShellKind::Bash,
        "zsh" => ChildShellKind::Zsh,
        _ => ChildShellKind::Other,
    }
}

/// bash / zsh 用の一時 rcfile を生成する。`Other` のときは `None`。
pub fn prepare_interactive_rc(shell_path: &str) -> io::Result<Option<ShellRcLayout>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    match detect_child_shell(shell_path) {
        ChildShellKind::Bash => {
            let dir = tempfile::tempdir()?;
            let rc = dir.path().join("aish-bashrc");
            let user_rc = PathBuf::from(&home).join(".bashrc");
            write_bash_wrapper(&rc, &user_rc)?;
            Ok(Some(ShellRcLayout {
                _dir: dir,
                bash_rcfile: Some(rc),
                zdotdir: None,
            }))
        }
        ChildShellKind::Zsh => {
            let dir = tempfile::tempdir()?;
            let zdot = dir.path().join("zdotdir");
            fs::create_dir(&zdot)?;
            let zshrc = zdot.join(".zshrc");
            let user_zshrc = PathBuf::from(&home).join(".zshrc");
            write_zsh_wrapper(&zshrc, &user_zshrc)?;
            Ok(Some(ShellRcLayout {
                _dir: dir,
                bash_rcfile: None,
                zdotdir: Some(zdot),
            }))
        }
        ChildShellKind::Other => Ok(None),
    }
}

pub struct ShellRcLayout {
    _dir: tempfile::TempDir,
    pub bash_rcfile: Option<PathBuf>,
    pub zdotdir: Option<PathBuf>,
}

fn write_bash_wrapper(dst: &Path, user_rc: &Path) -> io::Result<()> {
    let mut body = String::new();
    if user_rc.is_file() {
        body.push_str(&format!(
            "if [ -f {} ]; then . {}; fi\n",
            shell_quote(user_rc),
            shell_quote(user_rc)
        ));
    }
    body.push_str(BASH_SNIPPET);
    fs::write(dst, body)
}

fn write_zsh_wrapper(dst: &Path, user_zshrc: &Path) -> io::Result<()> {
    let mut body = String::new();
    if user_zshrc.is_file() {
        body.push_str(&format!(
            "if [ -f {} ]; then . {}; fi\n",
            shell_quote(user_zshrc),
            shell_quote(user_zshrc)
        ));
    }
    body.push_str(ZSH_SNIPPET);
    fs::write(dst, body)
}

fn shell_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("'{s}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bash_and_zsh() {
        assert_eq!(detect_child_shell("/bin/bash"), ChildShellKind::Bash);
        assert_eq!(detect_child_shell("/usr/bin/zsh"), ChildShellKind::Zsh);
        assert_eq!(detect_child_shell("/bin/fish"), ChildShellKind::Other);
    }

    #[test]
    fn bash_wrapper_includes_snippet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = dir.path().join("rc");
        write_bash_wrapper(&rc, Path::new("/nonexistent/.bashrc")).expect("write");
        let content = fs::read_to_string(rc).expect("read");
        assert!(content.contains("aish complete bash"));
    }
}
