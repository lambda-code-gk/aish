use crate::ports::outbound::{
    AgentStateLoader, AgentStateSaver, CommandAllowRulesLoader, ContinueAfterLimitPrompt,
    EventSinkFactory, InterruptChecker, LlmEventStreamFactory, ProfileLister, RunQuery,
    SessionHistoryLoader, SessionResponseSaver, ToolApproval,
};
use crate::usecase::agent_loop::{AgentLoop, AgentLoopOutcome};
use common::ports::outbound::EnvResolver;
use common::ports::outbound::{now_iso8601, FileSystem, Log, LogLevel, LogRecord, Process};
use common::error::Error;
use common::llm::provider::Message as LlmMessage;
use crate::domain::Query;
use common::msg::Msg;
use common::domain::SessionDir;
use common::tool::{Tool, ToolContext, ToolRegistry};
use std::sync::Arc;

/// 履歴 (Message) とクエリから Vec<Msg> を構築
fn build_messages(
    history_messages: &[LlmMessage],
    query: &Query,
    system_instruction: Option<&str>,
) -> Vec<Msg> {
    let mut msgs: Vec<Msg> = Vec::new();
    if let Some(s) = system_instruction {
        msgs.push(Msg::system(s));
    }
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
    msgs.push(Msg::user(query.as_ref()));
    msgs
}

/// ai のユースケース（アダプター経由で I/O を行う）
pub struct AiUseCase {
    pub fs: Arc<dyn FileSystem>,
    pub history_loader: Arc<dyn SessionHistoryLoader>,
    pub response_saver: Arc<dyn SessionResponseSaver>,
    pub agent_state_saver: Arc<dyn AgentStateSaver>,
    pub agent_state_loader: Arc<dyn AgentStateLoader>,
    pub continue_prompt: Arc<dyn ContinueAfterLimitPrompt>,
    pub env_resolver: Arc<dyn EnvResolver>,
    pub process: Arc<dyn Process>,
    pub command_allow_rules_loader: Arc<dyn CommandAllowRulesLoader>,
    pub sink_factory: Arc<dyn EventSinkFactory>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub approver: Arc<dyn ToolApproval>,
    pub interrupt_checker: Arc<dyn InterruptChecker>,
    pub log: Arc<dyn Log>,
    pub profile_lister: Arc<dyn ProfileLister>,
    pub llm_stream_factory: Arc<dyn LlmEventStreamFactory>,
}

impl AiUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fs: Arc<dyn FileSystem>,
        history_loader: Arc<dyn SessionHistoryLoader>,
        response_saver: Arc<dyn SessionResponseSaver>,
        agent_state_saver: Arc<dyn AgentStateSaver>,
        agent_state_loader: Arc<dyn AgentStateLoader>,
        continue_prompt: Arc<dyn ContinueAfterLimitPrompt>,
        env_resolver: Arc<dyn EnvResolver>,
        process: Arc<dyn Process>,
        command_allow_rules_loader: Arc<dyn CommandAllowRulesLoader>,
        sink_factory: Arc<dyn EventSinkFactory>,
        tools: Vec<Arc<dyn Tool>>,
        approver: Arc<dyn ToolApproval>,
        interrupt_checker: Arc<dyn InterruptChecker>,
        log: Arc<dyn Log>,
        profile_lister: Arc<dyn ProfileLister>,
        llm_stream_factory: Arc<dyn LlmEventStreamFactory>,
    ) -> Self {
        Self {
            fs,
            history_loader,
            response_saver,
            agent_state_saver,
            agent_state_loader,
            continue_prompt,
            env_resolver,
            process,
            command_allow_rules_loader,
            sink_factory,
            tools,
            approver,
            interrupt_checker,
            log,
            profile_lister,
            llm_stream_factory,
        }
    }

    pub(crate) fn session_is_valid(&self, session_dir: &Option<SessionDir>) -> bool {
        if let Some(ref dir) = session_dir {
            self.fs.exists(dir.as_ref()) && self.fs.metadata(dir.as_ref()).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            false
        }
    }

    /// 現在有効なプロファイル一覧を返す（ソート済み名前リストとデフォルトプロファイル名）。
    /// 表示は CLI の責務のため、usecase はデータのみ返す。
    pub fn list_profiles(&self) -> Result<(Vec<String>, Option<String>), Error> {
        self.profile_lister.list_profiles()
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

    /// エラー終了時に続き用状態を保存する（LLM エラー・クラッシュ時などに resume 可能にする）。
    /// 保存に失敗してもエラーにはせず、呼び出し元の Err をそのまま返す。
    fn try_save_agent_state_on_error(&self, session_dir: &Option<SessionDir>, messages: &[Msg]) {
        if messages.is_empty() || !self.session_is_valid(session_dir) {
            return;
        }
        if let Some(dir) = session_dir {
            if let Err(e) = self.agent_state_saver.save(dir, messages) {
                let _ = self.log.log(&LogRecord {
                    ts: now_iso8601(),
                    level: LogLevel::Warn,
                    message: format!("Failed to save agent state on error: {}", e),
                    layer: Some("usecase".to_string()),
                    kind: Some("error".to_string()),
                    fields: None,
                });
            }
        }
    }

    fn run_query_impl(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        model: Option<common::domain::ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        max_turns_override: Option<usize>,
    ) -> Result<i32, Error> {
        let _ = self.log.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "query started".to_string(),
            layer: Some("usecase".to_string()),
            kind: Some("usecase".to_string()),
            fields: None,
        });

        let messages: Vec<Msg> = match query {
            None => {
                // Resume: 保存された続き用状態から再開
                let dir = session_dir.as_ref().ok_or_else(|| {
                    Error::invalid_argument("No continuation state. Please provide a message.")
                })?;
                if !self.session_is_valid(&session_dir) {
                    return Err(Error::invalid_argument(
                        "No continuation state. Please provide a message.",
                    ));
                }
                self.agent_state_loader
                    .load(dir)?
                    .ok_or_else(|| {
                        Error::invalid_argument("No continuation state. Please provide a message.")
                    })?
            }
            Some(q) => {
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
                build_messages(&history_messages, q, system_instruction)
            }
        };

        let (stream, ctx) = match self.llm_stream_factory.create_stream(
            session_dir.as_ref(),
            provider.as_ref(),
            model.as_ref(),
            system_instruction,
        ) {
            Ok(s) => s,
            Err(e) => {
                self.try_save_agent_state_on_error(&session_dir, &messages);
                return Err(e);
            }
        };

        const DEFAULT_MAX_TURNS: usize = 16;
        let max_turns = max_turns_override.unwrap_or(DEFAULT_MAX_TURNS);
        let home_dir = match self.env_resolver.resolve_home_dir() {
            Ok(h) => h,
            Err(e) => {
                self.try_save_agent_state_on_error(&session_dir, &messages);
                return Err(e);
            }
        };
        let allow_rules = self.command_allow_rules_loader.load_rules(&home_dir);

        let mut messages = messages;
        let ctx = ctx.0;
        loop {
            let mut registry = ToolRegistry::new();
            for t in &self.tools {
                registry.register(Arc::clone(t));
            }
            let tool_context = ToolContext::new(
                session_dir.as_ref().map(|s: &SessionDir| s.as_ref().to_path_buf()),
            )
            .with_command_allow_rules(allow_rules.clone());
            let sinks = self.sink_factory.create_sinks();
            let mut agent_loop = AgentLoop::new(
                Arc::clone(&stream),
                registry,
                tool_context,
                sinks,
                Arc::clone(&self.approver),
                Some("run_shell"),
                Some(Arc::clone(&self.interrupt_checker)),
            );

            let outcome = match agent_loop
                .run_until_done(&messages, max_turns, max_turns)
                .map_err(|e| e.with_context(ctx.clone()))
            {
                Ok(o) => o,
                Err(e) => {
                    self.try_save_agent_state_on_error(&session_dir, &messages);
                    return Err(e);
                }
            };

            match outcome {
                AgentLoopOutcome::Done(_msgs, assistant_text) => {
                    if self.session_is_valid(&session_dir) {
                        let dir = session_dir.as_ref().expect("session_dir is Some");
                        self.agent_state_saver.clear(dir)?;
                        if !assistant_text.trim().is_empty() {
                            self.response_saver.save_assistant(dir, &assistant_text)?;
                            self.truncate_console_log(dir)?;
                        }
                    }
                    let _ = self.log.log(&LogRecord {
                        ts: now_iso8601(),
                        level: LogLevel::Info,
                        message: "query finished".to_string(),
                        layer: Some("usecase".to_string()),
                        kind: Some("usecase".to_string()),
                        fields: None,
                    });
                    return Ok(0);
                }
                AgentLoopOutcome::ReachedLimit(msgs, assistant_text) => {
                    let continue_ = match self.continue_prompt.ask_continue() {
                        Ok(c) => c,
                        Err(e) => {
                            self.try_save_agent_state_on_error(&session_dir, &msgs);
                            return Err(e);
                        }
                    };
                    if !continue_ {
                        if self.session_is_valid(&session_dir) {
                            let dir = session_dir.as_ref().expect("session_dir is Some");
                            self.agent_state_saver.save(dir, &msgs)?;
                            if !assistant_text.trim().is_empty() {
                                self.response_saver.save_assistant(dir, &assistant_text)?;
                                self.truncate_console_log(dir)?;
                            }
                        }
                        let _ = self.log.log(&LogRecord {
                            ts: now_iso8601(),
                            level: LogLevel::Info,
                            message: "query finished (saved for resume)".to_string(),
                            layer: Some("usecase".to_string()),
                            kind: Some("usecase".to_string()),
                            fields: None,
                        });
                        return Ok(0);
                    }
                    messages = msgs;
                }
            }
        }
    }
}

impl RunQuery for AiUseCase {
    fn run_query(
        &self,
        session_dir: Option<common::domain::SessionDir>,
        provider: Option<common::domain::ProviderName>,
        model: Option<common::domain::ModelName>,
        query: Option<&Query>,
        system_instruction: Option<&str>,
        max_turns_override: Option<usize>,
    ) -> Result<i32, Error> {
        self.run_query_impl(
            session_dir,
            provider,
            model,
            query,
            system_instruction,
            max_turns_override,
        )
    }
}

