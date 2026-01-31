use crate::adapter::{run_task_if_exists, StdoutSink};
use crate::cli::Config;
use crate::domain::History;
use crate::usecase::agent_loop::{AgentLoop, LlmEventStream};
use common::adapter::{FileSystem, Process};
use common::error::Error;
use common::llm::factory::AnyProvider;
use common::llm::{create_driver, LlmDriver, ProviderType};
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use common::part_id::IdGenerator;
use common::tool::{ToolContext, ToolRegistry};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

/// セッションディレクトリを環境変数から取得
fn session_dir_from_env() -> Option<PathBuf> {
    env::var("AISH_SESSION").ok().map(PathBuf::from)
}

/// ドライバを LlmEventStream として使うアダプタ
struct DriverLlmStream<'a>(&'a LlmDriver<AnyProvider>);

impl LlmEventStream for DriverLlmStream<'_> {
    fn stream_events(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[LlmMessage],
        callback: &mut dyn FnMut(common::llm::events::LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.0.query_stream_events(query, system_instruction, history, callback)
    }
}

/// 履歴 (Message) とクエリから Vec<Msg> を構築
fn build_messages(history_messages: &[LlmMessage], query: &str) -> Vec<Msg> {
    let mut msgs: Vec<Msg> = history_messages
        .iter()
        .map(|m| {
            if m.role == "user" {
                Msg::user(&m.content)
            } else {
                Msg::assistant(&m.content)
            }
        })
        .collect();
    msgs.push(Msg::user(query));
    msgs
}

/// ai のユースケース（アダプター経由で I/O を行う）
pub struct AiUseCase {
    pub fs: Arc<dyn FileSystem>,
    pub id_gen: Arc<dyn IdGenerator>,
    pub process: Arc<dyn Process>,
}

impl AiUseCase {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        id_gen: Arc<dyn IdGenerator>,
        process: Arc<dyn Process>,
    ) -> Self {
        Self { fs, id_gen, process }
    }

    fn session_is_valid(&self, session_dir: &Option<PathBuf>) -> bool {
        if let Some(ref dir) = session_dir {
            self.fs.exists(dir) && self.fs.metadata(dir).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            false
        }
    }

    pub(crate) fn load_history(&self, session_dir: &PathBuf) -> Result<History, Error> {
        if !self.fs.exists(session_dir) {
            return Ok(History::new());
        }
        if self
            .fs
            .metadata(session_dir)
            .map(|m| !m.is_dir())
            .unwrap_or(true)
        {
            return Ok(History::new());
        }
        let mut part_files: Vec<PathBuf> = self
            .fs
            .read_dir(session_dir)?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |s| s.starts_with("part_"))
                    && self.fs.metadata(path).map(|m| m.is_file()).unwrap_or(false)
            })
            .collect();
        part_files.sort();

        let mut history = History::new();
        for part_file in part_files {
            match self.fs.read_to_string(&part_file) {
                Ok(content) => {
                    if let Some(name_str) = part_file.file_name().and_then(|n| n.to_str()) {
                        if name_str.ends_with("_user.txt") {
                            history.push_user(content);
                        } else if name_str.ends_with("_assistant.txt") {
                            history.push_assistant(content);
                        } else {
                            eprintln!("Warning: Unknown part file type: {}", name_str);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to read part file '{}': {}", part_file.display(), e);
                }
            }
        }
        Ok(history)
    }

    pub(crate) fn save_response(&self, session_dir: &PathBuf, response: &str) -> Result<(), Error> {
        if !self.fs.exists(session_dir)
            || !self
                .fs
                .metadata(session_dir)
                .map(|m| m.is_dir())
                .unwrap_or(false)
        {
            return Err(Error::io_msg("Session is not valid"));
        }
        let id = self.id_gen.next_id();
        let filename = format!("part_{}_assistant.txt", id);
        let file_path = session_dir.join(&filename);
        self.fs.write(&file_path, response)
    }

    fn truncate_console_log(&self, session_dir: &PathBuf) -> Result<(), Error> {
        let args = vec![
            "-s".to_string(),
            session_dir.display().to_string(),
            "truncate_console_log".to_string(),
        ];
        let _ = self.process.run(std::path::Path::new("aish"), &args);
        Ok(())
    }

    pub fn run(&self, config: Config) -> Result<i32, Error> {
        let session_dir = session_dir_from_env();
        if config.help {
            print_help();
            return Ok(0);
        }

        if let Some(ref task_name) = config.task {
            if let Some(code) = run_task_if_exists(
                self.fs.as_ref(),
                self.process.as_ref(),
                task_name,
                &config.message_args,
            )? {
                return Ok(code);
            }
        }

        let mut query_parts = Vec::new();
        if let Some(ref task) = config.task {
            query_parts.push(task.clone());
        }
        query_parts.extend_from_slice(&config.message_args);

        if query_parts.is_empty() {
            return Err(Error::invalid_argument(
                "No query provided. Please provide a message to send to the LLM.",
            ));
        }

        let query = query_parts.join(" ");

        let history_messages = if self.session_is_valid(&session_dir) {
            let dir = session_dir.as_ref().unwrap();
            self.load_history(dir)
                .ok()
                .map(|h| h.messages().to_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let provider_type = if let Some(ref provider_name) = config.provider {
            ProviderType::from_str(provider_name.as_ref()).ok_or_else(|| {
                Error::invalid_argument(format!(
                    "Unknown provider: {}. Supported providers: gemini, gpt, echo",
                    provider_name
                ))
            })?
        } else {
            ProviderType::Gemini
        };

        let driver = create_driver(provider_type, None)?;
        let stream = DriverLlmStream(&driver);
        let sinks: Vec<Box<dyn common::sink::EventSink>> =
            vec![Box::new(StdoutSink::new())];
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(common::tool::EchoTool::new()));
        let tool_context = ToolContext::new(session_dir.as_ref().map(|p| p.clone()));
        let messages = build_messages(&history_messages, &query);
        let mut agent_loop =
            AgentLoop::new(stream, registry, tool_context, sinks);
        const MAX_TURNS: usize = 16;
        let (_final_messages, assistant_text) =
            agent_loop.run_until_done(&messages, MAX_TURNS)?;

        println!();

        if self.session_is_valid(&session_dir) && !assistant_text.trim().is_empty() {
            let dir = session_dir.as_ref().unwrap();
            self.save_response(dir, &assistant_text)?;
            self.truncate_console_log(dir)?;
        }

        Ok(0)
    }
}

/// 標準アダプターで AiUseCase を組み立てて run する（従来の run_app の入口）
pub fn run_app(config: Config) -> Result<i32, Error> {
    let fs = Arc::new(common::adapter::StdFileSystem);
    let id_gen = Arc::new(common::part_id::StdIdGenerator::new(Arc::new(
        common::adapter::StdClock,
    )));
    let process = Arc::new(common::adapter::StdProcess);
    let use_case = AiUseCase::new(fs, id_gen, process);
    use_case.run(config)
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
    use std::sync::Arc;

    fn use_case() -> AiUseCase {
        AiUseCase::new(
            Arc::new(common::adapter::StdFileSystem),
            Arc::new(common::part_id::StdIdGenerator::new(Arc::new(
                common::adapter::StdClock,
            ))),
            Arc::new(common::adapter::StdProcess),
        )
    }

    #[test]
    fn test_session_from_env_no_env_var() {
        let original = env::var("AISH_SESSION").ok();
        env::remove_var("AISH_SESSION");

        let session_dir = session_dir_from_env();
        assert!(session_dir.is_none());

        let uc = use_case();
        assert!(!uc.session_is_valid(&session_dir));

        if let Some(val) = original {
            env::set_var("AISH_SESSION", val);
        }
    }

    #[test]
    fn test_session_from_env_with_existing_dir() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_valid");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        fs::create_dir_all(&session_dir).unwrap();

        let original = env::var("AISH_SESSION").ok();
        env::set_var("AISH_SESSION", session_dir.to_str().unwrap());

        let session_dir_opt = session_dir_from_env();
        let uc = use_case();
        assert!(uc.session_is_valid(&session_dir_opt));
        assert_eq!(session_dir_opt.as_ref().unwrap(), &session_dir);

        if let Some(val) = original {
            env::set_var("AISH_SESSION", val);
        } else {
            env::remove_var("AISH_SESSION");
        }
        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_session_from_env_with_nonexistent_dir() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_nonexistent");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }

        let original = env::var("AISH_SESSION").ok();
        env::set_var("AISH_SESSION", session_dir.to_str().unwrap());

        let session_dir_opt = session_dir_from_env();
        let uc = use_case();
        assert!(!uc.session_is_valid(&session_dir_opt));

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
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No query provided"));
        assert_eq!(err.exit_code(), 64);
    }

    #[test]
    fn test_run_app_with_message() {
        // echoプロバイダを使用してネットワーク不要で高速に実行
        // （provider未指定だとGeminiが使われ、APIキー欠如でHTTPタイムアウトまで数秒かかる）
        let config = Config {
            provider: Some(ProviderName::new("echo")),
            message_args: vec!["Hello".to_string()],
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok(), "echo provider should succeed without API key");
    }

    #[test]
    fn test_run_app_with_task_and_message() {
        // echoプロバイダを使用（agentタスクは存在しない想定なのでLLMパスに入る）
        let config = Config {
            provider: Some(ProviderName::new("echo")),
            task: Some("agent".to_string()),
            message_args: vec!["hello".to_string(), "world".to_string()],
            ..Default::default()
        };
        let result = run_app(config);
        assert!(result.is_ok(), "echo provider should succeed without API key");
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
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unknown provider"));
        assert_eq!(err.exit_code(), 64);
    }

}

#[cfg(test)]
mod session_history_tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    fn use_case() -> AiUseCase {
        AiUseCase::new(
            Arc::new(common::adapter::StdFileSystem),
            Arc::new(common::part_id::StdIdGenerator::new(Arc::new(
                common::adapter::StdClock,
            ))),
            Arc::new(common::adapter::StdProcess),
        )
    }

    #[test]
    fn test_load_history_no_directory() {
        let temp_dir = std::env::temp_dir();
        let non_existent_dir = temp_dir.join("aish_test_nonexistent_session");

        let uc = use_case();
        let result = uc.load_history(&non_existent_dir);
        assert!(result.is_ok());
        let history = result.unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_load_history_empty_session_dir() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_empty_session");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        fs::create_dir_all(&session_dir).unwrap();

        let uc = use_case();
        let result = uc.load_history(&session_dir);
        assert!(result.is_ok());
        let history = result.unwrap();
        assert!(history.is_empty());

        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_load_history_with_files() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_with_files");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        fs::create_dir_all(&session_dir).unwrap();

        let part1 = session_dir.join("part_20240101_120000_user.txt");
        let part2 = session_dir.join("part_20240102_120000_assistant.txt");
        let part3 = session_dir.join("part_20240103_120000_user.txt");

        fs::write(&part1, "First part content").unwrap();
        fs::write(&part2, "Second part content").unwrap();
        fs::write(&part3, "Third part content").unwrap();

        let uc = use_case();
        let result = uc.load_history(&session_dir);
        assert!(result.is_ok());
        let history = result.unwrap();

        assert_eq!(history.messages().len(), 3);
        assert_eq!(history.messages()[0].role, "user");
        assert_eq!(history.messages()[0].content, "First part content");
        assert_eq!(history.messages()[1].role, "assistant");
        assert_eq!(history.messages()[1].content, "Second part content");
        assert_eq!(history.messages()[2].role, "user");
        assert_eq!(history.messages()[2].content, "Third part content");

        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_load_history_ignores_non_part_files() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_session_ignore_files");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        fs::create_dir_all(&session_dir).unwrap();

        let part_file = session_dir.join("part_20240101_120000_user.txt");
        let other_file = session_dir.join("other_file.txt");

        fs::write(&part_file, "Part content").unwrap();
        fs::write(&other_file, "Other content").unwrap();

        let uc = use_case();
        let result = uc.load_history(&session_dir);
        assert!(result.is_ok());
        let history = result.unwrap();

        assert_eq!(history.messages().len(), 1);
        assert_eq!(history.messages()[0].role, "user");
        assert_eq!(history.messages()[0].content, "Part content");

        fs::remove_dir_all(&session_dir).unwrap();
    }
}

#[cfg(test)]
mod save_response_tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    fn use_case() -> AiUseCase {
        AiUseCase::new(
            Arc::new(common::adapter::StdFileSystem),
            Arc::new(common::part_id::StdIdGenerator::new(Arc::new(
                common::adapter::StdClock,
            ))),
            Arc::new(common::adapter::StdProcess),
        )
    }

    #[test]
    fn test_save_response() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_save_response");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        fs::create_dir_all(&session_dir).unwrap();

        let uc = use_case();
        let response = "This is a test response from the assistant.";
        let result = uc.save_response(&session_dir, response);
        assert!(result.is_ok());

        let entries: Vec<_> = fs::read_dir(&session_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| {
                let name_str = name.to_str().unwrap();
                name_str.starts_with("part_") && name_str.ends_with("_assistant.txt")
            })
            .collect();

        assert_eq!(entries.len(), 1);
        let file_path = session_dir.join(&entries[0]);
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, response);

        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_save_response_with_user_part() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_save_response_with_user");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }
        fs::create_dir_all(&session_dir).unwrap();

        let user_part_id = "AZwJxha3";
        let user_part_file = session_dir.join(format!("part_{}_user.txt", user_part_id));
        fs::write(&user_part_file, "User message").unwrap();

        let uc = use_case();
        let response = "This is a test response from the assistant.";
        let result = uc.save_response(&session_dir, response);
        assert!(result.is_ok());

        let entries: Vec<_> = fs::read_dir(&session_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|name| {
                let name_str = name.to_str().unwrap();
                name_str.starts_with("part_") && name_str.ends_with("_assistant.txt")
            })
            .collect();

        assert_eq!(entries.len(), 1);
        let file_path = session_dir.join(&entries[0]);
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, response);

        fs::remove_dir_all(&session_dir).unwrap();
    }

    #[test]
    fn test_save_response_nonexistent_session() {
        let temp_dir = std::env::temp_dir();
        let session_dir = temp_dir.join("aish_test_nonexistent_save");

        if session_dir.exists() {
            fs::remove_dir_all(&session_dir).unwrap();
        }

        let uc = use_case();
        let response = "This is a test response.";
        let result = uc.save_response(&session_dir, response);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not valid"));
    }
}

