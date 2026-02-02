use crate::adapter::{load_command_allow_rules, run_task_if_exists, CliToolApproval, ShellTool, StdoutSink};
use crate::cli::{config_to_command, Config};
use crate::domain::{AiCommand, History};
use crate::ports::inbound::RunAiApp;
use crate::ports::outbound::{LlmEventStream, ToolApproval};
use crate::usecase::agent_loop::AgentLoop;
use common::ports::outbound::EnvResolver;
use common::{adapter::FileSystem, adapter::Process};
use common::error::Error;
use common::llm::factory::AnyProvider;
use common::llm::{create_driver, LlmDriver, ProviderType};
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use common::part_id::IdGenerator;
use common::domain::SessionDir;
use common::tool::{ToolContext, ToolRegistry};
use std::path::PathBuf;
use std::sync::Arc;

/// ドライバを LlmEventStream として使うアダプタ
struct DriverLlmStream<'a>(&'a LlmDriver<AnyProvider>);

impl LlmEventStream for DriverLlmStream<'_> {
    fn stream_events(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[LlmMessage],
        tools: Option<&[common::tool::ToolDef]>,
        callback: &mut dyn FnMut(common::llm::events::LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.0.query_stream_events(query, system_instruction, history, tools, callback)
    }
}

/// 履歴 (Message) とクエリから Vec<Msg> を構築
fn build_messages(history_messages: &[LlmMessage], query: &str) -> Vec<Msg> {
    let mut msgs: Vec<Msg> = Vec::new();
    for m in history_messages {
        if m.role == "user" {
            msgs.push(Msg::user(&m.content));
        } else if m.role == "tool" {
            if let Some(ref call_id) = m.tool_call_id {
                let name = m.tool_name.as_deref().unwrap_or("");
                msgs.push(Msg::tool_result(call_id, name, serde_json::from_str(&m.content).unwrap_or(serde_json::json!({}))));
            }
        } else {
            // assistant
            msgs.push(Msg::assistant(&m.content));
            if let Some(ref tool_calls) = m.tool_calls {
                for tc in tool_calls {
                    msgs.push(Msg::tool_call(&tc.id, &tc.name, tc.args.clone(), tc.thought_signature.clone()));
                }
            }
        }
    }
    msgs.push(Msg::user(query));
    msgs
}

/// ai のユースケース（アダプター経由で I/O を行う）
pub struct AiUseCase {
    pub fs: Arc<dyn FileSystem>,
    pub id_gen: Arc<dyn IdGenerator>,
    pub env_resolver: Arc<dyn EnvResolver>,
    pub process: Arc<dyn Process>,
}

impl AiUseCase {
    pub fn new(
        fs: Arc<dyn FileSystem>,
        id_gen: Arc<dyn IdGenerator>,
        env_resolver: Arc<dyn EnvResolver>,
        process: Arc<dyn Process>,
    ) -> Self {
        Self {
            fs,
            id_gen,
            env_resolver,
            process,
        }
    }

    pub(crate) fn session_is_valid(&self, session_dir: &Option<SessionDir>) -> bool {
        if let Some(ref dir) = session_dir {
            self.fs.exists(dir.as_ref()) && self.fs.metadata(dir.as_ref()).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            false
        }
    }

    pub(crate) fn load_history(&self, session_dir: &SessionDir) -> Result<History, Error> {
        if !self.fs.exists(session_dir.as_ref()) {
            return Ok(History::new());
        }
        if self
            .fs
            .metadata(session_dir.as_ref())
            .map(|m| !m.is_dir())
            .unwrap_or(true)
        {
            return Ok(History::new());
        }
        let mut part_files: Vec<PathBuf> = self
            .fs
            .read_dir(session_dir.as_ref())?
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

    pub(crate) fn save_response(&self, session_dir: &SessionDir, response: &str) -> Result<(), Error> {
        if !self.fs.exists(session_dir.as_ref())
            || !self
                .fs
                .metadata(session_dir.as_ref())
                .map(|m| m.is_dir())
                .unwrap_or(false)
        {
            return Err(Error::io_msg("Session is not valid"));
        }
        let id = self.id_gen.next_id();
        let filename = format!("part_{}_assistant.txt", id);
        let file_path = session_dir.as_path().join(&filename);
        self.fs.write(&file_path, response)
    }

    fn truncate_console_log(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let args = vec![
            "-s".to_string(),
            session_dir.as_path().display().to_string(),
            "truncate_console_log".to_string(),
        ];
        let _ = self.process.run(std::path::Path::new("aish"), &args);
        Ok(())
    }

    fn run_query(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        query: &str,
    ) -> Result<i32, Error> {
        let history_messages = if self.session_is_valid(&session_dir) {
            let dir = session_dir.as_ref().expect("session_dir is Some");
            self.load_history(dir)
                .ok()
                .map(|h| h.messages().to_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let provider_type = if let Some(ref provider_name) = provider {
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
        // ツール追加: Tool 実装を追加し、ここで registry.register(Arc::new(MyTool::new())) するだけ
        // ShellTool は OS 副作用を伴うため adapter 層に配置
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(common::tool::EchoTool::new()));
        registry.register(Arc::new(ShellTool::new()));

        let messages = build_messages(&history_messages, &query);
        let home_dir = self.env_resolver.resolve_home_dir()?;
        let allow_rules = load_command_allow_rules(&home_dir);
        let tool_context = ToolContext::new(session_dir.as_ref().map(|s: &SessionDir| s.as_ref().to_path_buf()))
            .with_command_allow_rules(allow_rules);
        let approver: Arc<dyn ToolApproval> = Arc::new(CliToolApproval::new());
        let mut agent_loop =
            AgentLoop::new(stream, registry, tool_context, sinks, approver);
        const MAX_TURNS: usize = 16;
        let (_final_messages, assistant_text) =
            agent_loop.run_until_done(&messages, MAX_TURNS)?;

        println!();

        if self.session_is_valid(&session_dir) && !assistant_text.trim().is_empty() {
            let dir = session_dir.as_ref().expect("session_dir is Some");
            self.save_response(dir, &assistant_text)?;
            self.truncate_console_log(dir)?;
        }

        Ok(0)
    }
}

impl RunAiApp for AiUseCase {
    fn run(&self, config: Config) -> Result<i32, Error> {
        let session_dir = self.env_resolver.session_dir_from_env();
        let cmd = config_to_command(config);

        match cmd {
            AiCommand::Help => {
                print_help();
                return Ok(0);
            }
            AiCommand::Task {
                name,
                args,
                provider,
            } => {
                if let Some(code) = run_task_if_exists(
                    self.fs.as_ref(),
                    self.process.as_ref(),
                    name.as_ref(),
                    &args,
                )? {
                    return Ok(code);
                }
                // タスクが見つからなかった -> Query として扱う
                let mut query_parts = vec![name.as_ref().to_string()];
                query_parts.extend(args);
                let query = query_parts.join(" ");
                if query.trim().is_empty() {
                    return Err(Error::invalid_argument(
                        "No query provided. Please provide a message to send to the LLM.",
                    ));
                }
                self.run_query(session_dir, provider, &query)
            }
            AiCommand::Query { provider, query } => {
                if query.trim().is_empty() {
                    return Err(Error::invalid_argument(
                        "No query provided. Please provide a message to send to the LLM.",
                    ));
                }
                self.run_query(session_dir, provider, &query)
            }
        }
    }
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


