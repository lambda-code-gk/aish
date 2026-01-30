use crate::args::Config;
use crate::task::run_task_if_exists;
use common::error::{Error, invalid_argument, io_error_default};
use common::llm::{create_driver, ProviderType};
use common::domain::PartId;
use common::llm::provider::Message as LlmMessage;
use std::io::{self, Write};
use std::env;
use std::path::PathBuf;
use std::fs;
use std::cell::RefCell;
use std::rc::Rc;

/// セッションヒストリー
#[derive(Debug, Clone)]
pub struct History {
    messages: Vec<LlmMessage>,
}

impl History {
    pub fn new() -> Self {
        History {
            messages: Vec::new(),
        }
    }
    
    pub fn push_user(&mut self, content: impl Into<String>) {
        self.messages.push(LlmMessage::user(content));
    }
    
    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.messages.push(LlmMessage::assistant(content));
    }
    
    pub fn messages(&self) -> &[LlmMessage] {
        &self.messages
    }
    
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

/// セッション管理構造体
///
/// `AISH_SESSION`環境変数からセッションディレクトリを管理します。
struct Session {
    dir: Option<PathBuf>,
}

impl Session {
    /// 環境変数からセッションを作成
    ///
    /// `AISH_SESSION`環境変数が設定されている場合、セッションディレクトリを取得します。
    /// 環境変数が設定されていない場合は、`None`を返します。
    fn from_env() -> Self {
        let dir = env::var("AISH_SESSION")
            .ok()
            .map(PathBuf::from);
        Session { dir }
    }

    /// セッションが有効かどうかを判定
    ///
    /// # Returns
    /// * `true` - セッションが有効（環境変数が設定され、ディレクトリが存在する）
    /// * `false` - セッションが無効（環境変数が設定されていない、またはディレクトリが存在しない）
    fn is_valid(&self) -> bool {
        if let Some(ref dir) = self.dir {
            dir.exists() && dir.is_dir()
        } else {
            false
        }
    }

    /// セッションディレクトリのパスを取得
    ///
    /// # Returns
    /// セッションディレクトリのパス、または`None`（セッションが無効な場合）
    fn dir(&self) -> Option<&PathBuf> {
        if self.is_valid() {
            self.dir.as_ref()
        } else {
            None
        }
    }

    /// セッションディレクトリからpartファイルを読み込んで履歴として返す
    ///
    /// # Returns
    /// セッションヒストリー、またはエラー
    pub fn load_history(&self) -> Result<History, Error> {
        let session_dir = match self.dir() {
            Some(dir) => dir,
            None => return Ok(History::new()),
        };
        
        // part_* パターンのファイルを取得（セッションディレクトリ直下）
        let mut part_files = Vec::new();
        let entries = fs::read_dir(session_dir)
            .map_err(|e| io_error_default(&format!("Failed to read session directory '{}': {}", session_dir.display(), e)))?;
        
        for entry in entries {
            let entry = entry.map_err(|e| io_error_default(&format!("Failed to read directory entry: {}", e)))?;
            let path = entry.path();
            
            // ファイル名がpart_で始まるファイルのみを対象
            if let Some(file_name) = path.file_name() {
                if let Some(name_str) = file_name.to_str() {
                    if name_str.starts_with("part_") && path.is_file() {
                        part_files.push(path);
                    }
                }
            }
        }
        
        // ファイル名順にソート
        part_files.sort();
        
        // 各ファイルの内容を読み込んでHistoryに追加
        let mut history = History::new();
        for part_file in part_files {
            match fs::read_to_string(&part_file) {
                Ok(content) => {
                    // ファイル名からメッセージタイプを判定
                    if let Some(file_name) = part_file.file_name() {
                        if let Some(name_str) = file_name.to_str() {
                            if name_str.ends_with("_user.txt") {
                                history.push_user(content);
                            } else if name_str.ends_with("_assistant.txt") {
                                history.push_assistant(content);
                            } else {
                                // part_*_user.txt でも part_*_assistant.txt でもない場合は警告
                                eprintln!("Warning: Unknown part file type: {}", name_str);
                            }
                        }
                    }
                }
                Err(e) => {
                    // ファイル読み込みエラーは警告として無視（次のファイルを続ける）
                    eprintln!("Warning: Failed to read part file '{}': {}", part_file.display(), e);
                }
            }
        }
        
        Ok(history)
    }

    /// レスポンスをpart_*_assistant.txtファイルとして保存する
    ///
    /// # Arguments
    /// * `response` - 保存するレスポンス内容
    ///
    /// # Returns
    /// 成功時はOk(())、エラー時はError
    pub fn save_response(&self, response: &str) -> Result<(), Error> {
        let session_dir = match self.dir() {
            Some(dir) => dir,
            None => return Err(io_error_default("Session is not valid")),
        };
        
        // 新しいIDを生成
        let id = PartId::generate();
        
        // ファイル名を生成: part_<ID>_assistant.txt
        let filename = format!("part_{}_assistant.txt", id);
        let file_path = session_dir.join(&filename);
        
        // ファイルに書き込み
        fs::write(&file_path, response)
            .map_err(|e| io_error_default(&format!("Failed to write response to '{}': {}", file_path.display(), e)))?;
        
        Ok(())
    }

    /// console.txtとaishのメモリバッファをトランケートする
    ///
    /// aiコマンドがレスポンスを保存した後、`aish truncate_console_log`を呼び出すことで、
    /// aishプロセスのメモリ上のバッファとconsole.txtファイルをクリアする。
    /// これにより、次のSIGUSR1でpart_*_user.txtが重複して作成されることを防ぐ。
    ///
    /// # Returns
    /// 成功時はOk(())、aishが実行中でない場合もOk(())を返す
    pub fn truncate_console_log(&self) -> Result<(), Error> {
        use std::process::Command;
        
        let session_dir = match self.dir() {
            Some(dir) => dir,
            None => return Ok(()), // セッションが無効な場合は何もしない
        };
        
        // aish truncate_console_log を呼び出す
        // -s オプションでセッションディレクトリを指定
        let result = Command::new("aish")
            .arg("-s")
            .arg(session_dir)
            .arg("truncate_console_log")
            .output();
        
        match result {
            Ok(output) => {
                if !output.status.success() {
                    // aishコマンドがエラーを返した場合は警告を出力するが、処理は続行
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.is_empty() {
                        eprintln!("Warning: aish truncate_console_log failed: {}", stderr.trim());
                    }
                }
            }
            Err(e) => {
                // aishコマンドが見つからない場合などは警告を出力するが、処理は続行
                eprintln!("Warning: Failed to run aish truncate_console_log: {}", e);
            }
        }
        
        Ok(())
    }

}

pub fn run_app(config: Config) -> Result<i32, Error> {
    // セッションを初期化
    let session = Session::from_env();
    if config.help {
        print_help();
        return Ok(0);
    }

    // まず task があれば、対応するタスクスクリプト/execute を探して実行する
    if let Some(ref task_name) = config.task {
        if let Some(code) = run_task_if_exists(task_name, &config.message_args)? {
            return Ok(code);
        }
    }

    // タスクが見つからなかった場合は、従来通りLLMクエリとして扱う
    let mut query_parts = Vec::new();
    if let Some(ref task) = config.task {
        query_parts.push(task.clone());
    }
    query_parts.extend_from_slice(&config.message_args);

    if query_parts.is_empty() {
        return Err(invalid_argument(
            "No query provided. Please provide a message to send to the LLM.",
        ));
    }

    let query = query_parts.join(" ");

    // セッションが有効な場合、セッションヒストリーを読み込む
    let history_messages = if session.is_valid() {
        session.load_history()
            .ok()
            .map(|history| history.messages().to_vec())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // プロバイダタイプを決定
    let provider_type = if let Some(ref provider_name) = config.provider {
        ProviderType::from_str(&*provider_name).ok_or_else(|| {
            invalid_argument(&format!(
                "Unknown provider: {}. Supported providers: gemini, gpt, echo",
                provider_name
            ))
        })?
    } else {
        // デフォルトはGemini
        ProviderType::Gemini
    };

    // ドライバーを作成（デフォルトモデルを使用）
    let driver = create_driver(provider_type, None)?;

    // レスポンスを収集するためのバッファ（セッションが有効な場合に保存用）
    // Rc<RefCell<String>>を使用して、クロージャと共有できるようにする
    let response_buffer = if session.is_valid() {
        Some(Rc::new(RefCell::new(String::new())))
    } else {
        None
    };

    // LLMにクエリを送信（ストリーミング表示）
    driver.query_streaming(
        &query,
        None,
        &history_messages,
        {
            // Rcをクローンすることで、同じRefCell<String>への参照を共有
            let buffer = response_buffer.clone();
            Box::new(move |chunk| {
                print!("{}", chunk);
                io::stdout()
                    .flush()
                    .map_err(|e| io_error_default(&format!("Failed to flush stdout: {}", e)))?;
                
                // バッファに蓄積
                if let Some(ref buf) = buffer {
                    buf.borrow_mut().push_str(chunk);
                }
                
                Ok(())
            })
        },
    )?;

    // 最後に改行を出力
    println!();

    // セッションが有効な場合、レスポンスをpart_*_assistant.txtとして保存
    if session.is_valid() {
        if let Some(buffer) = response_buffer {
            // Rc::try_unwrapでRefCellを取り出し、into_inner()で内部の値を取得
            let response = match Rc::try_unwrap(buffer) {
                Ok(cell) => cell.into_inner(),
                Err(rc) => rc.borrow().clone(),
            };
            if !response.trim().is_empty() {
                session.save_response(&response)?;
                // aiコマンドの出力がpart_*_user.txtとして重複保存されないように、
                // console.txtをトランケートする
                session.truncate_console_log()?;
            }
        }
    }

    Ok(0)
}

/// 引数不正時に stderr へ出力する usage 行（main から呼ぶ）
pub fn print_usage() {
    eprintln!("Usage: ai [options] [task] [message...]");
}

fn print_help() {
    println!("Usage: ai [options] [task] [message...]");
    println!("Options:");
    println!("  -h, --help                    Show this help message");
    println!("  -p, --provider <provider>      Specify LLM provider (gemini, gpt, echo). Default: gemini");
    println!();
    println!("Description:");
    println!("  Send a message to the LLM and display the response.");
    println!("  If a matching task script exists, execute it instead of sending a query.");
    println!();
    println!("Task search paths:");
    println!("  $AISH_HOME/config/task.d/");
    println!("  $XDG_CONFIG_HOME/aish/task.d");
    println!();
    println!("Examples:");
    println!("  ai Hello, how are you?");
    println!("  ai -p gpt What is Rust programming language?");
    println!("  ai --provider echo Explain quantum computing");
    println!("  ai mytask do something");
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_session_from_env_no_env_var() {
        // AISH_SESSION環境変数を一時的に削除
        let original = env::var("AISH_SESSION").ok();
        env::remove_var("AISH_SESSION");
        
        let session = Session::from_env();
        // 環境変数が設定されていない場合、dirはNoneになる
        assert!(session.dir.is_none());
        assert!(!session.is_valid());
        assert!(session.dir().is_none());
        
        // 環境変数を復元
        if let Some(val) = original {
            env::set_var("AISH_SESSION", val);
        }
    }

    #[test]
    fn test_session_from_env_with_existing_dir() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_valid");
        
        // クリーンアップ
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        // セッションディレクトリを作成
        fs::create_dir_all(&session_dir).unwrap();
        
        // 環境変数を設定
        let original = env::var("AISH_SESSION").ok();
        env::set_var("AISH_SESSION", session_dir.to_str().unwrap());
        
        let session = Session::from_env();
        assert!(session.is_valid());
        assert!(session.dir().is_some());
        assert_eq!(session.dir().unwrap(), &session_dir);
        
        // 環境変数を復元
        if let Some(val) = original {
            env::set_var("AISH_SESSION", val);
        } else {
            env::remove_var("AISH_SESSION");
        }
        
        // クリーンアップ
        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_session_from_env_with_nonexistent_dir() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_nonexistent");
        
        // セッションディレクトリが存在しないことを確認
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        // 環境変数を設定
        let original = env::var("AISH_SESSION").ok();
        env::set_var("AISH_SESSION", session_dir.to_str().unwrap());
        
        let session = Session::from_env();
        assert!(!session.is_valid());
        assert!(session.dir().is_none());
        
        // 環境変数を復元
        if let Some(val) = original {
            env::set_var("AISH_SESSION", val);
        } else {
            env::remove_var("AISH_SESSION");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::domain::ProviderName;

    #[test]
    fn test_run_app_with_help() {
        let config = Config {
            help: true,
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_run_app_without_query() {
        let config = Config::default();
        let result = run_app(config);
        // クエリがない場合はエラー
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("No query provided"));
        assert_eq!(code, 64);
    }

    #[test]
    fn test_run_app_with_message() {
        // 環境変数が設定されていない場合はエラーになるが、基本的な構造はテストできる
        let config = Config {
            message_args: vec!["Hello".to_string()],
            ..Default::default()
        };
        // APIキーがない場合はエラーになるが、引数解析は正常
        let _result = run_app(config);
        // 環境変数がない場合はエラーになる
        // 実際の動作確認は統合テストで行う
    }

    #[test]
    fn test_run_app_with_task_and_message() {
        // 環境変数が設定されていない場合はエラーになるが、基本的な構造はテストできる
        let config = Config {
            task: Some("agent".to_string()),
            message_args: vec!["hello".to_string(), "world".to_string()],
            ..Default::default()
        };
        // APIキーがない場合はエラーになるが、引数解析は正常
        let _result = run_app(config);
        // 環境変数がない場合はエラーになる
        // 実際の動作確認は統合テストで行う
    }

    #[test]
    fn test_run_app_help_takes_precedence() {
        let config = Config {
            help: true,
            task: Some("agent".to_string()),
            message_args: vec!["hello".to_string()],
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_run_app_with_provider() {
        // 環境変数が設定されていない場合はエラーになるが、基本的な構造はテストできる
        let config = Config {
            provider: Some(ProviderName::new("echo")),
            message_args: vec!["Hello".to_string()],
            ..Default::default()
        };
        // EchoプロバイダはAPIキーが不要なので成功する
        let result = run_app(config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_app_with_unknown_provider() {
        let config = Config {
            provider: Some(ProviderName::new("unknown")),
            message_args: vec!["Hello".to_string()],
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("Unknown provider"));
        assert_eq!(code, 64);
    }

}

#[cfg(test)]
mod session_history_tests {
    use super::*;
    use std::fs;

    fn create_session_with_dir(dir: &PathBuf) -> Session {
        Session { dir: Some(dir.clone()) }
    }

    #[test]
    fn test_load_history_no_directory() {
        let temp_dir = std::env::temp_dir();
        let non_existent_dir = temp_dir.join("aish_test_nonexistent_session");
        
        let session = create_session_with_dir(&non_existent_dir);
        let result = session.load_history();
        assert!(result.is_ok());
        let history = result.unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_load_history_empty_session_dir() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_empty_session");
        
        // クリーンアップ
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        // セッションディレクトリを作成（partファイルなし）
        fs::create_dir_all(&session_dir).unwrap();
        
        let session = create_session_with_dir(&session_dir);
        let result = session.load_history();
        assert!(result.is_ok());
        let history = result.unwrap();
        assert!(history.is_empty());
        
        // クリーンアップ
        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_load_history_with_files() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_with_files");
        
        // クリーンアップ
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        // セッションディレクトリを作成
        fs::create_dir_all(&session_dir).unwrap();
        
        // partファイルを作成（セッションディレクトリ直下、ファイル名順にソートされることを確認するため、順序を変えて作成）
        let part1 = session_dir.join("part_20240101_120000_user.txt");
        let part2 = session_dir.join("part_20240102_120000_assistant.txt");
        let part3 = session_dir.join("part_20240103_120000_user.txt");
        
        fs::write(&part1, "First part content").unwrap();
        fs::write(&part2, "Second part content").unwrap();
        fs::write(&part3, "Third part content").unwrap();
        
        let session = create_session_with_dir(&session_dir);
        let result = session.load_history();
        assert!(result.is_ok());
        let history = result.unwrap();
        
        // メッセージが3つあることを確認
        assert_eq!(history.messages().len(), 3);
        
        // メッセージタイプを確認
        assert_eq!(history.messages()[0].role, "user");
        assert_eq!(history.messages()[0].content, "First part content");
        assert_eq!(history.messages()[1].role, "assistant");
        assert_eq!(history.messages()[1].content, "Second part content");
        assert_eq!(history.messages()[2].role, "user");
        assert_eq!(history.messages()[2].content, "Third part content");
        
        // クリーンアップ
        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_load_history_ignores_non_part_files() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_ignore_files");
        
        // クリーンアップ
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        // セッションディレクトリを作成
        fs::create_dir_all(&session_dir).unwrap();
        
        // part_で始まるファイルと、そうでないファイルを作成（セッションディレクトリ直下）
        let part_file = session_dir.join("part_20240101_120000_user.txt");
        let other_file = session_dir.join("other_file.txt");
        
        fs::write(&part_file, "Part content").unwrap();
        fs::write(&other_file, "Other content").unwrap();
        
        let session = create_session_with_dir(&session_dir);
        let result = session.load_history();
        assert!(result.is_ok());
        let history = result.unwrap();
        
        // メッセージが1つあることを確認
        assert_eq!(history.messages().len(), 1);
        assert_eq!(history.messages()[0].role, "user");
        assert_eq!(history.messages()[0].content, "Part content");
        
        // クリーンアップ
        fs::remove_dir_all(&session_dir).unwrap();
    }

}

#[cfg(test)]
mod save_response_tests {
    use super::*;
    use std::fs;

    fn create_session_with_dir(dir: &PathBuf) -> Session {
        Session { dir: Some(dir.clone()) }
    }

    #[test]
    fn test_save_response() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_save_response");
        
        // クリーンアップ
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        // セッションディレクトリを作成
        fs::create_dir_all(&session_dir).unwrap();
        
        let session = create_session_with_dir(&session_dir);
        let response = "This is a test response from the assistant.";
        let result = session.save_response(response);
        assert!(result.is_ok());
        
        // ファイルが作成されたことを確認
        let entries: Vec<_> = fs::read_dir(&session_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| {
                let name_str = name.to_str().unwrap();
                name_str.starts_with("part_") && name_str.ends_with("_assistant.txt")
            })
            .collect();
        
        assert_eq!(entries.len(), 1);
        
        // ファイルの内容を確認
        let file_path = session_dir.join(&entries[0]);
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, response);
        
        // クリーンアップ
        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_save_response_with_user_part() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_save_response_with_user");
        
        // クリーンアップ
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        // セッションディレクトリを作成
        fs::create_dir_all(&session_dir).unwrap();
        
        // まず、part_*_user.txtファイルを作成
        let user_part_id = "AZwJxha3";
        let user_part_file = session_dir.join(format!("part_{}_user.txt", user_part_id));
        fs::write(&user_part_file, "User message").unwrap();
        
        let session = create_session_with_dir(&session_dir);
        let response = "This is a test response from the assistant.";
        let result = session.save_response(response);
        assert!(result.is_ok());
        
        // part_*_assistant.txtファイルが作成されたことを確認（新しいIDが生成される）
        let entries: Vec<_> = fs::read_dir(&session_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| {
                let name_str = name.to_str().unwrap();
                name_str.starts_with("part_") && name_str.ends_with("_assistant.txt")
            })
            .collect();
        
        assert_eq!(entries.len(), 1);
        
        // ファイルの内容を確認
        let file_path = session_dir.join(&entries[0]);
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, response);
        
        // クリーンアップ
        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_save_response_nonexistent_session() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_nonexistent_save");
        
        // セッションディレクトリが存在しないことを確認
        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        
        let session = create_session_with_dir(&session_dir);
        let response = "This is a test response.";
        let result = session.save_response(response);
        assert!(result.is_err());
        let (msg, _code) = result.unwrap_err();
        assert!(msg.contains("not valid"));
    }

}

