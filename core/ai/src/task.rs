use std::env;
use std::path::{Path, PathBuf};

use common::adapter::{FileSystem, Process};
use common::error::Error;

/// タスクを解決して実行する（アダプター経由）
///
/// - `$AISH_HOME/config/task.d/` を最優先で探索
/// - それが無ければ `$XDG_CONFIG_HOME/aish/task.d/` を探索
/// - `task_name.sh` または `task_name/execute` が存在すればそれを実行する
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
        let dir = Path::new(&aish_home).join("config").join("task.d");
        if fs.exists(&dir) {
            if let Ok(m) = fs.metadata(&dir) {
                if m.is_dir() {
                    return Some(dir);
                }
            }
        }
    }

    if let Ok(xdg_config_home) = env::var("XDG_CONFIG_HOME") {
        let dir = Path::new(&xdg_config_home).join("aish").join("task.d");
        if fs.exists(&dir) {
            if let Ok(m) = fs.metadata(&dir) {
                if m.is_dir() {
                    return Some(dir);
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

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::{StdFileSystem, StdProcess};
    use std::fs::{self, File};
    use std::io::Write;

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

    #[test]
    fn test_run_task_not_found() {
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
}


