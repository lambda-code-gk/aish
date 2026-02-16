use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::error::Error;
use common::ports::outbound::{FileSystem, Process};

use crate::ports::outbound::TaskRunner;

/// TaskRunner の標準実装（run_task_if_exists をラップ）
pub struct StdTaskRunner {
    fs: Arc<dyn FileSystem>,
    process: Arc<dyn Process>,
}

impl StdTaskRunner {
    pub fn new(fs: Arc<dyn FileSystem>, process: Arc<dyn Process>) -> Self {
        Self { fs, process }
    }
}

impl TaskRunner for StdTaskRunner {
    fn run_if_exists(&self, task_name: &str, args: &[String]) -> Result<Option<i32>, Error> {
        run_task_if_exists(self.fs.as_ref(), self.process.as_ref(), task_name, args)
    }

    fn list_names(&self) -> Result<Vec<String>, Error> {
        list_task_names(self.fs.as_ref())
    }
}

/// タスクを解決して実行する（アダプター経由）
///
/// task.d の検索順:
/// - AISH_HOME 設定時: `$AISH_HOME/config/task.d/` または `$AISH_HOME/task.d/`（先に存在する方）
///   （aish から渡る AISH_HOME が aish config ルートのときは task.d 直下）
/// - `$XDG_CONFIG_HOME/aish/task.d/`（XDG_CONFIG_HOME 設定時）
/// - `~/.config/aish/task.d/`（上記未設定時のデフォルト）
/// タスクは `task_name.sh` または `task_name/execute` で解決する。
///
/// 戻り値:
/// - `Ok(Some(code))`  : タスクを実行し、その終了コードを返した
/// - `Ok(None)`       : タスクが見つからなかった（呼び出し元で他の処理を行う）
/// - `Err(Error)`     : 実行時エラー
pub fn run_task_if_exists<F, P>(
    fs: &F,
    process: &P,
    task_name: &str,
    args: &[String],
) -> Result<Option<i32>, Error>
where
    F: FileSystem + ?Sized,
    P: Process + ?Sized,
{
    if task_name.is_empty() {
        return Ok(None);
    }

    let task_dir = match find_task_dir(fs) {
        Some(dir) => dir,
        None => return Ok(None),
    };

    let resolved = resolve_task_path(fs, &task_dir, task_name);
    let task_path = match resolved {
        Some(path) => path,
        None => return Ok(None),
    };

    let exit_status = process.run(&task_path, args)?;
    Ok(Some(exit_status))
}

fn find_task_dir<F: FileSystem + ?Sized>(fs: &F) -> Option<PathBuf> {
    if let Ok(aish_home) = env::var("AISH_HOME") {
        if !aish_home.is_empty() {
            let base = Path::new(&aish_home);
            // 2 通り: AISH_HOME が「ルート」のとき config/task.d、aish config ルートのとき task.d 直下
            for candidate in [base.join("config").join("task.d"), base.join("task.d")] {
                if fs.exists(&candidate) {
                    if let Ok(m) = fs.metadata(&candidate) {
                        if m.is_dir() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
    }

    if let Ok(xdg_config_home) = env::var("XDG_CONFIG_HOME") {
        if !xdg_config_home.is_empty() {
            let dir = Path::new(&xdg_config_home).join("aish").join("task.d");
            if fs.exists(&dir) {
                if let Ok(m) = fs.metadata(&dir) {
                    if m.is_dir() {
                        return Some(dir);
                    }
                }
            }
        }
    }

    // AISH_HOME / XDG_CONFIG_HOME 未設定時: ~/.config/aish/task.d（新構成のデフォルト）
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            let dir = Path::new(&home).join(".config").join("aish").join("task.d");
            if fs.exists(&dir) {
                if let Ok(m) = fs.metadata(&dir) {
                    if m.is_dir() {
                        return Some(dir);
                    }
                }
            }
        }
    }

    None
}

fn resolve_task_path<F: FileSystem + ?Sized>(fs: &F, task_dir: &Path, task_name: &str) -> Option<PathBuf> {
    let dir_execute = task_dir.join(task_name).join("execute");
    if fs.exists(&dir_execute) {
        if let Ok(m) = fs.metadata(&dir_execute) {
            if m.is_file() {
                return Some(dir_execute);
            }
        }
    }

    let script = task_dir.join(format!("{}.sh", task_name));
    if fs.exists(&script) {
        if let Ok(m) = fs.metadata(&script) {
            if m.is_file() {
                return Some(script);
            }
        }
    }

    None
}

/// タスク名一覧を返す（task.d 内のディレクトリ名と .sh のベース名）。補完用。
fn list_task_names<F: FileSystem + ?Sized>(fs: &F) -> Result<Vec<String>, Error> {
    let task_dir = match find_task_dir(fs) {
        Some(dir) => dir,
        None => return Ok(Vec::new()),
    };

    let entries = fs.read_dir(&task_dir)?;
    let mut names = Vec::new();

    for path in entries {
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let full = task_dir.join(name);
        let meta = match fs.metadata(&full) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            if fs.exists(&full.join("execute")) {
                if let Ok(m) = fs.metadata(&full.join("execute")) {
                    if m.is_file() {
                        names.push(name.to_string());
                    }
                }
            }
        } else if meta.is_file() && name.ends_with(".sh") {
            let base = name.strip_suffix(".sh").unwrap_or(name);
            names.push(base.to_string());
        }
    }

    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::{StdFileSystem, StdProcess};
    use std::fs::{self, File};
    use std::io::Write;
    use std::sync::Mutex;

    /// 環境変数 AISH_HOME / XDG_CONFIG_HOME を触るテストが並列で動くと競合するため、直列化する
    static TASK_ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn create_temp_dir(prefix: &str) -> PathBuf {
        let base = env::temp_dir();
        let unique = format!("{}_{}", prefix, std::process::id());
        let dir = base.join(unique);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_run_task_if_exists_no_env() {
        let _guard = TASK_ENV_MUTEX.lock().unwrap();
        env::remove_var("AISH_HOME");
        env::remove_var("XDG_CONFIG_HOME");

        let fs = StdFileSystem;
        let process = StdProcess;
        let result = run_task_if_exists(&fs, &process, "test", &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_run_task_script_in_aish_home() {
        let _guard = TASK_ENV_MUTEX.lock().unwrap();
        let tmp = create_temp_dir("aish_task_test_script");
        let config_dir = tmp.join("config").join("task.d");
        fs::create_dir_all(&config_dir).unwrap();

        env::set_var("AISH_HOME", &tmp);
        env::remove_var("XDG_CONFIG_HOME");

        let script_path = config_dir.join("hello.sh");
        let mut file = File::create(&script_path).unwrap();
        writeln!(file, "#!/usr/bin/env bash").unwrap();
        writeln!(file, "echo \"script executed with args: $@\" >> \"{}/output.txt\"", tmp.display()).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).unwrap();
        }

        let fs = StdFileSystem;
        let dir = find_task_dir(&fs).expect("task dir should be found");
        let resolved = resolve_task_path(&fs, &dir, "hello").expect("task should be resolved");
        assert_eq!(resolved, script_path);
    }

    #[test]
    fn test_run_task_execute_in_xdg_config_home() {
        let _guard = TASK_ENV_MUTEX.lock().unwrap();
        let tmp = create_temp_dir("aish_task_test_execute");
        let config_home = tmp.join("config");
        let task_dir = config_home.join("aish").join("task.d").join("mytask");
        fs::create_dir_all(&task_dir).unwrap();

        env::remove_var("AISH_HOME");
        env::set_var("XDG_CONFIG_HOME", &config_home);

        let execute_path = task_dir.join("execute");
        let mut file = File::create(&execute_path).unwrap();
        writeln!(file, "#!/usr/bin/env bash").unwrap();
        writeln!(file, "echo \"execute called\" >> \"{}/exec.txt\"", tmp.display()).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&execute_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&execute_path, perms).unwrap();
        }

        let fs = StdFileSystem;
        let dir = find_task_dir(&fs).expect("task dir should be found");
        let resolved = resolve_task_path(&fs, &dir, "mytask").expect("task should be resolved");
        assert_eq!(resolved, execute_path);
    }

    /// AISH_HOME が aish config ルート（task.d が直下、.sandbox/xdg/config/aish 相当）のときタスクを解決する
    #[test]
    fn test_find_task_dir_when_aish_home_is_config_root() {
        let _guard = TASK_ENV_MUTEX.lock().unwrap();
        let tmp = create_temp_dir("aish_task_test_config_root");
        let aish_config_root = tmp.join("xdg").join("config").join("aish");
        let task_d = aish_config_root.join("task.d");
        fs::create_dir_all(&task_d).unwrap();
        let script = task_d.join("commit_staged.sh");
        File::create(&script).unwrap();

        env::set_var("AISH_HOME", &aish_config_root);
        env::remove_var("XDG_CONFIG_HOME");

        let fs = StdFileSystem;
        let dir = find_task_dir(&fs).expect("task dir should be found when AISH_HOME is config root");
        assert_eq!(dir, task_d);
        let resolved = resolve_task_path(&fs, &dir, "commit_staged").expect("task should resolve");
        assert_eq!(resolved, script);
    }

    #[test]
    fn test_run_task_not_found() {
        let _guard = TASK_ENV_MUTEX.lock().unwrap();
        let tmp = create_temp_dir("aish_task_test_not_found");
        let config_dir = tmp.join("config").join("task.d");
        fs::create_dir_all(&config_dir).unwrap();

        env::set_var("AISH_HOME", &tmp);
        env::remove_var("XDG_CONFIG_HOME");

        let fs = StdFileSystem;
        let process = StdProcess;
        let result = run_task_if_exists(&fs, &process, "unknown_task", &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_list_task_names() {
        let _guard = TASK_ENV_MUTEX.lock().unwrap();
        let tmp = create_temp_dir("aish_task_test_list");
        let config_dir = tmp.join("config").join("task.d");
        fs::create_dir_all(&config_dir).unwrap();

        // task as script: foo.sh
        let foo_sh = config_dir.join("foo.sh");
        File::create(&foo_sh).unwrap();
        // task as dir with execute: bar/execute
        let bar_dir = config_dir.join("bar");
        fs::create_dir_all(&bar_dir).unwrap();
        let bar_exec = bar_dir.join("execute");
        File::create(&bar_exec).unwrap();

        env::set_var("AISH_HOME", &tmp);
        env::remove_var("XDG_CONFIG_HOME");

        let fs = StdFileSystem;
        let names = list_task_names(&fs).unwrap();
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"bar".to_string()));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_list_task_names_finds_default_config_aish_task_d() {
        let _guard = TASK_ENV_MUTEX.lock().unwrap();
        let tmp = create_temp_dir("aish_task_test_default");
        let aish_task_d = tmp.join(".config").join("aish").join("task.d");
        fs::create_dir_all(&aish_task_d).unwrap();
        let script = aish_task_d.join("hello.sh");
        File::create(&script).unwrap();

        let orig_home = env::var("HOME").ok();
        let orig_aish = env::var("AISH_HOME").ok();
        let orig_xdg = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("HOME", &tmp);
        env::remove_var("AISH_HOME");
        env::remove_var("XDG_CONFIG_HOME");

        let fs = StdFileSystem;
        let names = list_task_names(&fs).unwrap();
        assert!(names.contains(&"hello".to_string()), "names={:?}", names);

        if let Some(h) = orig_home {
            env::set_var("HOME", h);
        } else {
            env::remove_var("HOME");
        }
        if let Some(h) = orig_aish {
            env::set_var("AISH_HOME", h);
        } else {
            env::remove_var("AISH_HOME");
        }
        if let Some(h) = orig_xdg {
            env::set_var("XDG_CONFIG_HOME", h);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }
    }
}


