use crate::ports::outbound::{
    CommandAllowRulesLoader, EventSinkFactory, LlmEventStream, RunQuery, SessionHistoryLoader,
    SessionResponseSaver, ToolApproval,
};
use crate::usecase::agent_loop::AgentLoop;
use common::ports::outbound::EnvResolver;
use common::ports::outbound::{FileSystem, Process};
use common::error::Error;
use common::llm::factory::AnyProvider;
use common::llm::{create_driver, LlmDriver, ProviderType};
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use common::domain::SessionDir;
use common::tool::{Tool, ToolContext, ToolRegistry};
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
    pub history_loader: Arc<dyn SessionHistoryLoader>,
    pub response_saver: Arc<dyn SessionResponseSaver>,
    pub env_resolver: Arc<dyn EnvResolver>,
    pub process: Arc<dyn Process>,
    pub command_allow_rules_loader: Arc<dyn CommandAllowRulesLoader>,
    pub sink_factory: Arc<dyn EventSinkFactory>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub approver: Arc<dyn ToolApproval>,
}

impl AiUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fs: Arc<dyn FileSystem>,
        history_loader: Arc<dyn SessionHistoryLoader>,
        response_saver: Arc<dyn SessionResponseSaver>,
        env_resolver: Arc<dyn EnvResolver>,
        process: Arc<dyn Process>,
        command_allow_rules_loader: Arc<dyn CommandAllowRulesLoader>,
        sink_factory: Arc<dyn EventSinkFactory>,
        tools: Vec<Arc<dyn Tool>>,
        approver: Arc<dyn ToolApproval>,
    ) -> Self {
        Self {
            fs,
            history_loader,
            response_saver,
            env_resolver,
            process,
            command_allow_rules_loader,
            sink_factory,
            tools,
            approver,
        }
    }

    pub(crate) fn session_is_valid(&self, session_dir: &Option<SessionDir>) -> bool {
        if let Some(ref dir) = session_dir {
            self.fs.exists(dir.as_ref()) && self.fs.metadata(dir.as_ref()).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            false
        }
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

    fn run_query_impl(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        query: &str,
    ) -> Result<i32, Error> {
        let history_messages = if self.session_is_valid(&session_dir) {
            let dir = session_dir.as_ref().expect("session_dir is Some");
            self.history_loader
                .load(dir)
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
        let mut registry = ToolRegistry::new();
        for t in &self.tools {
            registry.register(Arc::clone(t));
        }

        let messages = build_messages(&history_messages, &query);
        let home_dir = self.env_resolver.resolve_home_dir()?;
        let allow_rules = self.command_allow_rules_loader.load_rules(&home_dir);
        let tool_context = ToolContext::new(session_dir.as_ref().map(|s: &SessionDir| s.as_ref().to_path_buf()))
            .with_command_allow_rules(allow_rules);
        let sinks = self.sink_factory.create_sinks();
        let mut agent_loop =
            AgentLoop::new(stream, registry, tool_context, sinks, Arc::clone(&self.approver), Some("run_shell"));
        const MAX_TURNS: usize = 16;
        let (_final_messages, assistant_text) =
            agent_loop.run_until_done(&messages, MAX_TURNS)?;

        println!();

        if self.session_is_valid(&session_dir) && !assistant_text.trim().is_empty() {
            let dir = session_dir.as_ref().expect("session_dir is Some");
            self.response_saver.save_assistant(dir, &assistant_text)?;
            self.truncate_console_log(dir)?;
        }

        Ok(0)
    }
}

impl RunQuery for AiUseCase {
    fn run_query(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        query: &str,
    ) -> Result<i32, Error> {
        self.run_query_impl(session_dir, provider, query)
    }
}

