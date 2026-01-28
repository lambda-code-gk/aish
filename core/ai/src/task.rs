use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::error::{Error, io_error_default};

/// タスクを解決して実行する
///
/// - `$AISH_HOME/config/task.d/` を最優先で探索
/// - それが無ければ `$XDG_CONFIG_HOME/aish/task.d/` を探索
/// - `task_name.sh` または `task_name/execute` が存在すればそれを実行する
///
/// 戻り値:
/// - `Ok(Some(code))`  : タスクを実行し、その終了コードを返した
/// - `Ok(None)`       : タスクが見つからなかった（呼び出し元で他の処理を行う）
/// - `Err((msg,code))`: 実行時エラー
pub fn run_task_if_exists(task_name: &str, args: &[String]) -> Result<Option<i32>, Error> {
    if task_name.is_empty() {
        return Ok(None);
    }

    let task_dir = match find_task_dir() {
        Some(dir) => dir,
        None => return Ok(None),
    };

    let resolved = resolve_task_path(&task_dir, task_name);
    let task_path = match resolved {
        Some(path) => path,
        None => return Ok(None),
    };

    let exit_status = execute_task(&task_path, args)?;
    Ok(Some(exit_status))
}

fn find_task_dir() -> Option<PathBuf> {
    // 1. $AISH_HOME/config/task.d
    if let Ok(aish_home) = env::var("AISH_HOME") {
        let dir = Path::new(&aish_home).join("config").join("task.d");
        if dir.is_dir() {
            return Some(dir);
        }
    }

    // 2. $XDG_CONFIG_HOME/aish/task.d
    if let Ok(xdg_config_home) = env::var("XDG_CONFIG_HOME") {
        let dir = Path::new(&xdg_config_home).join("aish").join("task.d");
        if dir.is_dir() {
            return Some(dir);
        }
    }

    None
}

fn resolve_task_path(task_dir: &Path, task_name: &str) -> Option<PathBuf> {
    // 1. ディレクトリ形式: task_name/execute
    let dir_execute = task_dir.join(task_name).join("execute");
    if dir_execute.is_file() {
        return Some(dir_execute);
    }

    // 2. スクリプト形式: task_name.sh
    let script = task_dir.join(format!("{}.sh", task_name));
    if script.is_file() {
        return Some(script);
    }

    None
}

fn execute_task(task_path: &Path, args: &[String]) -> Result<i32, Error> {
    // 念のため実行ビットが無くても実行できるようにする（プラットフォーム依存）
    // 失敗しても致命的ではないのでエラーは無視する
    if let Ok(metadata) = fs::metadata(task_path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = metadata.permissions();
            let mode = perms.mode();
            if mode & 0o111 == 0 {
                perms.set_mode(mode | 0o755);
                let _ = fs::set_permissions(task_path, perms);
            }
        }
    }

    let mut cmd = Command::new(task_path);
    cmd.args(args);

    let status = cmd.status().map_err(|e| {
        let msg = format!(
            "Failed to execute task '{}': {}",
            task_path.display(),
            io_error_to_string(&e)
        );
        io_error_default(&msg)
    })?;

    Ok(status.code().unwrap_or(1))
}

fn io_error_to_string(err: &io::Error) -> String {
    format!("{}", err)
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let result = run_task_if_exists("test", &[]);
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

        // 実行テストではなく、解決ロジックのみを検証する
        let dir = find_task_dir().expect("task dir should be found");
        let resolved = resolve_task_path(&dir, "hello").expect("task should be resolved");
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

        // 実行テストではなく、解決ロジックのみを検証する
        let dir = find_task_dir().expect("task dir should be found");
        let resolved = resolve_task_path(&dir, "mytask").expect("task should be resolved");
        assert_eq!(resolved, execute_path);
    }

    #[test]
    fn test_run_task_not_found() {
        let tmp = create_temp_dir("aish_task_test_not_found");
        let config_dir = tmp.join("config").join("task.d");
        fs::create_dir_all(&config_dir).unwrap();

        env::set_var("AISH_HOME", &tmp);
        env::remove_var("XDG_CONFIG_HOME");

        let result = run_task_if_exists("unknown_task", &[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}


