//! フックを実行してシステムプロンプトを解決する標準実装
//!
//! システム・ユーザー・プロジェクトの 3 種類の hooks/system_prompt を順に実行し、
//! 各 stdout を `\n\n` で結合する。

use crate::ports::outbound::ResolveSystemPromptFromHooks;
use common::error::Error;
use common::ports::outbound::{EnvResolver, FileSystem};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

const AISH_DIR: &str = ".aish";
const HOOKS_SUBDIR: &str = "hooks";
const SYSTEM_PROMPT_HOOK: &str = "system_prompt";

pub struct StdResolveSystemPromptFromHooks {
    env: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
}

impl StdResolveSystemPromptFromHooks {
    pub fn new(env: Arc<dyn EnvResolver>, fs: Arc<dyn FileSystem>) -> Self {
        Self { env, fs }
    }
}

impl ResolveSystemPromptFromHooks for StdResolveSystemPromptFromHooks {
    fn resolve_system_prompt_from_hooks(&self) -> Result<Option<String>, Error> {
        let mut parts = Vec::new();

        // 1. システム: $AISH_HOME/config/hooks/system_prompt または $XDG_CONFIG_HOME/aish/hooks/system_prompt
        let dirs = self.env.resolve_dirs()?;
        let system_dir = dirs.config_dir.join(HOOKS_SUBDIR).join(SYSTEM_PROMPT_HOOK);
        if let Some(s) = run_hook_dir(self.fs.as_ref(), &system_dir)? {
            parts.push(s);
        }

        // 2. ユーザー: $HOME/.aish/hooks/system_prompt
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                let user_dir = PathBuf::from(&home).join(AISH_DIR).join(HOOKS_SUBDIR).join(SYSTEM_PROMPT_HOOK);
                if let Some(s) = run_hook_dir(self.fs.as_ref(), &user_dir)? {
                    parts.push(s);
                }
            }
        }

        // 3. プロジェクト: {PROJECT_ROOT}/.aish/hooks/system_prompt
        let current = self.env.current_dir()?;
        if let Some(project_root) = find_project_root(current.as_path())? {
            let project_dir = project_root.join(AISH_DIR).join(HOOKS_SUBDIR).join(SYSTEM_PROMPT_HOOK);
            if let Some(s) = run_hook_dir(self.fs.as_ref(), &project_dir)? {
                parts.push(s);
            }
        }

        if parts.is_empty() {
            return Ok(None);
        }
        let combined = parts.join("\n\n");
        let trimmed = combined.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    }
}

/// カレントから上に遡り、.aish が存在するディレクトリ（プロジェクトルート）を返す。
fn find_project_root(mut current: &Path) -> Result<Option<PathBuf>, Error> {
    loop {
        let aish_dir = current.join(AISH_DIR);
        if aish_dir.exists() {
            if let Ok(meta) = std::fs::metadata(&aish_dir) {
                if meta.is_dir() {
                    return Ok(Some(current.to_path_buf()));
                }
            }
        }
        match current.parent() {
            Some(p) => current = p,
            None => return Ok(None),
        }
    }
}

/// 指定ディレクトリ内の実行可能ファイルを名前昇順で実行し、stdout を結合して返す。
/// ディレクトリが無い・空・全スクリプトが非ゼロ終了の場合は None。
/// 非ゼロ終了したスクリプトの出力は無視する（捨てて続行）。
fn run_hook_dir(fs: &dyn FileSystem, dir: &Path) -> Result<Option<String>, Error> {
    if !fs.exists(dir) {
        return Ok(None);
    }
    let entries = match fs.read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let mut executables: Vec<PathBuf> = entries
        .into_iter()
        .filter(|p| is_executable_file(fs, p))
        .collect();
    executables.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut outputs = Vec::new();
    for path in executables {
        if let Ok(output) = Command::new(&path).output() {
            if output.status.success() {
                if let Ok(s) = String::from_utf8(output.stdout) {
                    let t = s.trim();
                    if !t.is_empty() {
                        outputs.push(t.to_string());
                    }
                }
            }
        }
    }
    if outputs.is_empty() {
        Ok(None)
    } else {
        Ok(Some(outputs.join("\n\n")))
    }
}

fn is_executable_file(fs: &dyn FileSystem, path: &Path) -> bool {
    let meta = match fs.metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(std_meta) = std::fs::metadata(path) {
            return std_meta.permissions().mode() & 0o111 != 0;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::{StdEnvResolver, StdFileSystem};
    use std::fs;

    #[test]
    fn test_find_project_root_none() {
        let tmp = std::env::temp_dir().join("hook_find_root_none");
        let _ = std::fs::create_dir_all(&tmp);
        let r = find_project_root(tmp.as_path()).unwrap();
        assert!(r.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_project_root_finds_aish() {
        let tmp = std::env::temp_dir().join("hook_find_root_find");
        let _ = std::fs::create_dir_all(tmp.join(".aish").join("hooks").join("system_prompt"));
        let r = find_project_root(tmp.as_path()).unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap(), tmp);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[cfg(unix)]
    fn test_resolve_system_prompt_from_hooks_project_hook() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root = tmp.path();
        let hook_dir = project_root.join(".aish").join("hooks").join("system_prompt");
        fs::create_dir_all(&hook_dir).expect("create hook dir");
        let script = hook_dir.join("01_echo.sh");
        fs::write(&script, "#!/bin/sh\necho 'You are a helpful assistant.'\n").expect("write script");
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("chmod");

        let env: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
        let fs_adapter: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let resolver = StdResolveSystemPromptFromHooks::new(env, fs_adapter);

        let cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(project_root).expect("set_current_dir");

        let out = resolver.resolve_system_prompt_from_hooks().expect("resolve");
        let _ = std::env::set_current_dir(&cwd);
        assert!(out.is_some());
        assert_eq!(out.unwrap().trim(), "You are a helpful assistant.");
    }
}
