use crate::ports::outbound::{
    AgentStateLoader, AgentStateSaver, CommandAllowRulesLoader, ContextMessageBuilder,
    ContinueAfterLimitPrompt, EventSinkFactory, InterruptChecker, LlmEventStreamFactory,
    PrepareSessionForSensitiveCheck, ProfileLister, QueryPlacement, ResolveMemoryDir,
    ResolveProfileAndModel, RunQuery, SessionHistoryLoader, SessionResponseSaver, ToolApproval,
};
use crate::usecase::agent_loop::{AgentLoop, AgentLoopOutcome};
use common::ports::outbound::EnvResolver;
use common::ports::outbound::{now_iso8601, FileSystem, Log, LogLevel, LogRecord, Process};
use common::error::Error;
use crate::domain::Query;
use common::msg::Msg;
use common::domain::SessionDir;
use common::tool::{Tool, ToolContext, ToolRegistry};
use std::sync::Arc;

// --- 責務別 Deps（usecase が定義を所有し、wiring は組み立てるだけ）

pub struct AiDeps {
    pub session: SessionDeps,
    pub policy: PolicyDeps,
    pub tooling: ToolingDeps,
    pub model: ModelDeps,
    pub system: SystemDeps,
    pub obs: ObsDeps,
}

pub struct SessionDeps {
    pub fs: Arc<dyn FileSystem>,
    pub history_loader: Arc<dyn SessionHistoryLoader>,
    pub context_message_builder: Arc<dyn ContextMessageBuilder>,
    pub response_saver: Arc<dyn SessionResponseSaver>,
    pub agent_state_saver: Arc<dyn AgentStateSaver>,
    pub agent_state_loader: Arc<dyn AgentStateLoader>,
    pub prepare_session_for_sensitive_check: Option<Arc<dyn PrepareSessionForSensitiveCheck>>,
}

pub struct PolicyDeps {
    pub continue_prompt: Arc<dyn ContinueAfterLimitPrompt>,
    pub env_resolver: Arc<dyn EnvResolver>,
    pub resolve_memory_dir: Arc<dyn ResolveMemoryDir>,
    pub command_allow_rules_loader: Arc<dyn CommandAllowRulesLoader>,
    pub approver: Arc<dyn ToolApproval>,
    pub interrupt_checker: Arc<dyn InterruptChecker>,
}

pub struct ToolingDeps {
    pub sink_factory: Arc<dyn EventSinkFactory>,
    pub tools: Vec<Arc<dyn Tool>>,
}

pub struct ModelDeps {
    pub profile_lister: Arc<dyn ProfileLister>,
    pub resolve_profile_and_model: Arc<dyn ResolveProfileAndModel>,
    pub llm_stream_factory: Arc<dyn LlmEventStreamFactory>,
}

pub struct SystemDeps {
    pub process: Arc<dyn Process>,
}

pub struct ObsDeps {
    pub log: Arc<dyn Log>,
}

/// ai のユースケース（アダプター経由で I/O を行う）
pub struct AiUseCase {
    deps: AiDeps,
}

impl AiUseCase {
    pub fn new(deps: AiDeps) -> Self {
        Self { deps }
    }

    pub(crate) fn session_is_valid(&self, session_dir: &Option<SessionDir>) -> bool {
        if let Some(ref dir) = session_dir {
            self.deps.session.fs.exists(dir.as_ref()) && self.deps.session.fs.metadata(dir.as_ref()).map(|m| m.is_dir()).unwrap_or(false)
        } else {
            false
        }
    }

    /// 現在有効なプロファイル一覧を返す（ソート済み名前リストとデフォルトプロファイル名）。
    /// 表示は CLI の責務のため、usecase はデータのみ返す。
    pub fn list_profiles(&self) -> Result<(Vec<String>, Option<String>), Error> {
        self.deps.model.profile_lister.list_profiles()
    }

    /// 有効なツール一覧を返す（名前と説明）。プロバイダごとの有効/無効は未対応のため常に全ツール。
    /// 表示は CLI の責務のため、usecase はデータのみ返す。
    pub fn list_tools(&self) -> Vec<(String, String)> {
        self.deps.tooling.tools
            .iter()
            .map(|t| (t.name().to_string(), t.description().to_string()))
            .collect()
    }

    fn truncate_console_log(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let args = vec![
            "-s".to_string(),
            session_dir.as_path().display().to_string(),
            "truncate_console_log".to_string(),
        ];
        let _ = self.deps.system.process.run(std::path::Path::new("aish"), &args);
        Ok(())
    }

    /// エラー終了時に続き用状態を保存する（LLM エラー・クラッシュ時などに resume 可能にする）。
    /// 保存に失敗してもエラーにはせず、呼び出し元の Err をそのまま返す。
    fn try_save_agent_state_on_error(&self, session_dir: &Option<SessionDir>, messages: &[Msg]) {
        if messages.is_empty() || !self.session_is_valid(session_dir) {
            return;
        }
        if let Some(dir) = session_dir {
            if let Err(e) = self.deps.session.agent_state_saver.save(dir, messages) {
                let _ = self.deps.obs.log.log(&LogRecord {
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
        let (profile_name, model_name) = self
            .deps.model.resolve_profile_and_model
            .resolve(provider.as_ref(), model.as_ref())?;
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("event".to_string(), serde_json::json!("query_started"));
        fields.insert("profile".to_string(), serde_json::json!(profile_name.clone()));
        fields.insert("model".to_string(), serde_json::json!(model_name.clone()));
        let _ = self.deps.obs.log.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: format!(
                "query started (profile: {}, model: {})",
                profile_name, model_name
            ),
            layer: Some("usecase".to_string()),
            kind: Some("query".to_string()),
            fields: Some(fields),
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
                self.deps.session.agent_state_loader
                    .load(dir)?
                    .ok_or_else(|| {
                        Error::invalid_argument("No continuation state. Please provide a message.")
                    })?
            }
            Some(q) => {
                let (history_messages, query_placement) = if self.session_is_valid(&session_dir) {
                    let dir = session_dir.as_ref().expect("session_dir is Some");
                    self.deps.session.response_saver.save_user(dir, q.as_ref())?;
                    if let Some(ref prep) = self.deps.session.prepare_session_for_sensitive_check {
                        prep.prepare(dir)?;
                    }
                    let loaded = self.deps.session.history_loader.load(dir);
                    match loaded {
                        Ok(history) => {
                            // valid session: save_user → load しているので query は履歴に含まれている
                            (history.messages().to_vec(), QueryPlacement::AlreadyInHistory)
                        }
                        Err(_) => {
                            // load 失敗時でも query を欠落させないため、空履歴 + 末尾追加にフォールバック
                            (Vec::new(), QueryPlacement::AppendAtEnd)
                        }
                    }
                } else {
                    (Vec::new(), QueryPlacement::AppendAtEnd)
                };
                self.deps.session.context_message_builder.build(
                    &history_messages,
                    Some(q),
                    system_instruction,
                    query_placement,
                )
            }
        };

        let (stream, ctx) = match self.deps.model.llm_stream_factory.create_stream(
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
        let home_dir = match self.deps.policy.env_resolver.resolve_home_dir() {
            Ok(h) => h,
            Err(e) => {
                self.try_save_agent_state_on_error(&session_dir, &messages);
                return Err(e);
            }
        };
        let allow_rules = self.deps.policy.command_allow_rules_loader.load_rules(&home_dir);

        let mut messages = messages;
        let ctx = ctx.0;
        loop {
            let mut registry = ToolRegistry::new();
            for t in &self.deps.tooling.tools {
                registry.register(Arc::clone(t));
            }
            let (memory_project, memory_global) = match self.deps.policy.resolve_memory_dir.resolve() {
                Ok((p, g)) => (p, Some(g)),
                Err(_) => (None, None),
            };
            let tool_context = ToolContext::new(
                session_dir.as_ref().map(|s: &SessionDir| s.as_ref().to_path_buf()),
            )
            .with_command_allow_rules(allow_rules.clone())
            .with_memory_dirs(memory_project, memory_global);
            let sinks = self.deps.tooling.sink_factory.create_sinks();
            let mut agent_loop = AgentLoop::new(
                Arc::clone(&stream),
                registry,
                tool_context,
                sinks,
                Arc::clone(&self.deps.policy.approver),
                Some("run_shell"),
                Some(Arc::clone(&self.deps.policy.interrupt_checker)),
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
                        self.deps.session
                            .agent_state_saver
                            .clear_resume_keep_pending(dir)?;
                        if !assistant_text.trim().is_empty() {
                            self.deps.session.response_saver.save_assistant(dir, &assistant_text)?;
                            self.truncate_console_log(dir)?;
                        }
                    }
                    let _ = self.deps.obs.log.log(&LogRecord {
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
                    let continue_ = match self.deps.policy.continue_prompt.ask_continue() {
                        Ok(c) => c,
                        Err(e) => {
                            self.try_save_agent_state_on_error(&session_dir, &msgs);
                            return Err(e);
                        }
                    };
                    if !continue_ {
                        if self.session_is_valid(&session_dir) {
                            let dir = session_dir.as_ref().expect("session_dir is Some");
                            self.deps.session.agent_state_saver.save(dir, &msgs)?;
                            if !assistant_text.trim().is_empty() {
                                self.deps.session.response_saver.save_assistant(dir, &assistant_text)?;
                                self.truncate_console_log(dir)?;
                            }
                        }
                        let _ = self.deps.obs.log.log(&LogRecord {
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

