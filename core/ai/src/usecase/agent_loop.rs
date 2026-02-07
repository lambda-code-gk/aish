//! AgentLoop: イベント解釈器 + 状態機械
//!
//! 直列の transaction script をやめ、RunState で遷移する。
//! LLM から ToolCallEnd が来たら tool 実行フェーズへ遷移し、結果を messages に注入する。

use crate::ports::outbound::{Approval, LlmEventStream, ToolApproval};
use common::error::Error;
use common::llm::events::{FinishReason, LlmEvent};
use common::llm::provider::Message;
use common::msg::Msg;
use common::sink::{AgentEvent, EventSink};
use common::tool::{is_command_allowed, ToolContext, ToolDef, ToolRegistry};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// 実行状態
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// LLM ストリーム受信中
    StreamingModel,
    /// ツール実行中
    ExecutingTools,
    /// 正常終了
    Done,
    /// エラー終了（将来の LlmEvent::Failed 処理で使用）
    #[allow(dead_code)]
    Error,
}

/// エージェントループの終了結果（Done と上限到達を区別する）
#[derive(Debug, Clone)]
pub enum AgentLoopOutcome {
    /// 正常終了（LLM が Stop 等で終了）
    Done(Vec<Msg>, String),
    /// 最大ターン数に達したが会話は継続可能
    ReachedLimit(Vec<Msg>, String),
}

/// Vec<Msg> をドライバ用 (system_instruction, query, history) に変換
/// ToolCall/ToolResult は Assistant(content, tool_calls) と Tool(call_id, result) に変換
pub fn msgs_to_provider(msgs: &[Msg]) -> (Option<String>, String, Vec<Message>) {
    let mut system: Option<String> = None;
    let mut list: Vec<Message> = Vec::new();
    let mut last_user: Option<String> = None;
    let mut pending_assistant: Option<String> = None;
    let mut pending_tool_calls: Vec<(String, String, Value, Option<String>)> = Vec::new();

    fn flush_assistant_with_tool_calls(
        list: &mut Vec<Message>,
        pending_assistant: &mut Option<String>,
        pending_tool_calls: &mut Vec<(String, String, Value, Option<String>)>,
    ) {
        if pending_assistant.is_some() || !pending_tool_calls.is_empty() {
            let content = pending_assistant.take().unwrap_or_default();
            let tool_calls = std::mem::take(pending_tool_calls);
            list.push(Message::assistant_with_tool_calls(content, tool_calls));
        }
    }

    for m in msgs {
        match m {
            Msg::System(s) => {
                if system.is_none() {
                    system = Some(s.clone());
                }
            }
            Msg::User(s) => {
                flush_assistant_with_tool_calls(&mut list, &mut pending_assistant, &mut pending_tool_calls);
                last_user = Some(s.clone());
                list.push(Message::user(s));
            }
            Msg::Assistant(s) => {
                flush_assistant_with_tool_calls(&mut list, &mut pending_assistant, &mut pending_tool_calls);
                pending_assistant = Some(s.clone());
            }
            Msg::ToolCall { call_id, name, args, thought_signature } => {
                if pending_assistant.is_none() {
                    pending_assistant = Some(String::new());
                }
                pending_tool_calls.push((call_id.clone(), name.clone(), args.clone(), thought_signature.clone()));
            }
            Msg::ToolResult { call_id, name, result } => {
                flush_assistant_with_tool_calls(&mut list, &mut pending_assistant, &mut pending_tool_calls);
                let content = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                list.push(Message::tool_result(call_id.clone(), name.clone(), content));
            }
        }
    }
    flush_assistant_with_tool_calls(&mut list, &mut pending_assistant, &mut pending_tool_calls);

    // 最後が User なら query に分離、そうでなければ継続呼び出しなので query="" で history を全文
    let last_is_user = msgs.last().map(|m| matches!(m, Msg::User(_))).unwrap_or(false);
    let (query, history) = if last_is_user && last_user.is_some() {
        let q = last_user.as_ref().map(String::clone).unwrap_or_default();
        let h = list.iter().take(list.len().saturating_sub(1)).cloned().collect();
        (q, h)
    } else {
        (String::new(), list)
    };
    (system, query, history)
}

/// ストリーム中に蓄積するツール呼び出し（call_id -> (name, args_json_fragments, thought_signature)）
#[allow(dead_code)] // completed は将来の複数ツール蓄積用
struct ToolCallAccumulator {
    current_id: Option<String>,
    current_name: Option<String>,
    current_thought_signature: Option<String>,
    args_fragments: String,
    completed: Vec<(String, String, Value, Option<String>)>,
}

impl ToolCallAccumulator {
    fn new() -> Self {
        Self {
            current_id: None,
            current_name: None,
            current_thought_signature: None,
            args_fragments: String::new(),
            completed: Vec::new(),
        }
    }

    fn on_begin(&mut self, call_id: String, name: String, thought_signature: Option<String>) {
        self.current_id = Some(call_id);
        self.current_name = Some(name);
        self.current_thought_signature = thought_signature;
        self.args_fragments.clear();
    }

    fn on_args_delta(&mut self, fragment: String) {
        self.args_fragments.push_str(&fragment);
    }

    fn on_end(&mut self, call_id: String) -> Result<Option<(String, String, Value, Option<String>)>, Error> {
        let name = self.current_name.take().unwrap_or_default();
        let thought_signature = self.current_thought_signature.take();
        self.current_id = None;
        let args = if self.args_fragments.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&self.args_fragments)
                .map_err(|e| Error::json(format!("Invalid tool args JSON: {}", e)))?
        };
        self.args_fragments.clear();
        Ok(Some((call_id, name, args, thought_signature)))
    }
}

/// AgentLoop: 状態機械で LlmEvent を処理し、Sink に流す
pub struct AgentLoop<S: LlmEventStream> {
    stream: S,
    tool_registry: ToolRegistry,
    tool_context: ToolContext,
    sinks: Vec<Box<dyn EventSink>>,
    approver: Arc<dyn ToolApproval>,
    /// シェル系ツールの名前（allowlist 不一致時に承認を求める）。例: "run_shell"
    shell_tool_name: Option<&'static str>,
}

impl<S: LlmEventStream> AgentLoop<S> {
    pub fn new(
        stream: S,
        tool_registry: ToolRegistry,
        tool_context: ToolContext,
        sinks: Vec<Box<dyn EventSink>>,
        approver: Arc<dyn ToolApproval>,
        shell_tool_name: Option<&'static str>,
    ) -> Self {
        Self {
            stream,
            tool_registry,
            tool_context,
            sinks,
            approver,
            shell_tool_name,
        }
    }

    fn emit(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        for s in &mut self.sinks {
            s.on_event(ev)?;
        }
        Ok(())
    }

    fn emit_end(&mut self) -> Result<(), Error> {
        for s in &mut self.sinks {
            s.on_end()?;
        }
        Ok(())
    }

    /// 1 ターン実行: messages を元に LLM を呼び、イベントを Sink に流す。
    /// 受信したイベントは即座に Sink へ emit し、ストリーミング表示する。
    /// 戻り値: (new_messages, run_state, assistant_text)
    pub fn run_once(
        &mut self,
        messages: &[Msg],
    ) -> Result<(Vec<Msg>, RunState, String), Error> {
        let (system_opt, query, history) = msgs_to_provider(messages);
        let system_instruction = system_opt.as_deref();
        let tool_defs = self.tool_registry.list_definitions();
        let tools_ref = if tool_defs.is_empty() {
            None
        } else {
            Some(tool_defs.as_slice())
        };
        let collected: Rc<RefCell<Vec<LlmEvent>>> = Rc::new(RefCell::new(Vec::new()));
        let collected_inner = collected.clone();
        let sinks = &mut self.sinks;
        let mut cb = |ev: LlmEvent| -> Result<(), Error> {
            for s in sinks.iter_mut() {
                s.on_event(&AgentEvent::Llm(ev.clone()))?;
            }
            collected_inner.borrow_mut().push(ev);
            Ok(())
        };
        self.stream
            .stream_events(&query, system_instruction, &history, tools_ref, &mut cb)?;

        let mut assistant_text = String::new();
        let mut accumulator = ToolCallAccumulator::new();
        let mut pending_tool_calls: Vec<(String, String, Value, Option<String>)> = Vec::new();
        let mut run_state = RunState::StreamingModel;

        for ev in collected.borrow().iter() {
            match ev {
                LlmEvent::TextDelta(s) => assistant_text.push_str(s),
                LlmEvent::ToolCallBegin { call_id, name, thought_signature } => {
                    accumulator.on_begin(call_id.clone(), name.clone(), thought_signature.clone());
                }
                LlmEvent::ToolCallArgsDelta { json_fragment, .. } => {
                    accumulator.on_args_delta(json_fragment.clone());
                }
                LlmEvent::ToolCallEnd { call_id } => {
                    if let Some(tc) = accumulator.on_end(call_id.clone())? {
                        pending_tool_calls.push(tc);
                    }
                    run_state = RunState::ExecutingTools;
                }
                LlmEvent::Completed { .. } => {
                    if run_state != RunState::ExecutingTools {
                        run_state = RunState::Done;
                    }
                }
                LlmEvent::Failed { message } => return Err(Error::http(message.clone())),
            }
        }

        let mut new_messages = messages.to_vec();
        // ツール呼び出しがあった場合は、その前のテキストも含めて一つの Assistant ターンとして扱う
        if !assistant_text.is_empty() || !pending_tool_calls.is_empty() {
            new_messages.push(Msg::assistant(assistant_text.clone()));
        }

        if run_state == RunState::ExecutingTools && !pending_tool_calls.is_empty() {
            for (call_id, name, args, thought_signature) in pending_tool_calls {
                // 履歴にツール呼び出し自体を記録（直前の assistant メッセージに紐付く）
                new_messages.push(Msg::tool_call(call_id.clone(), name.clone(), args.clone(), thought_signature.clone()));

                // シェル系ツールの場合は allowlist 判定と承認を行う
                let effective_ctx = if self.shell_tool_name.map_or(false, |s| s == name.as_str()) {
                    let command = args.get("command").and_then(Value::as_str).unwrap_or("");
                    if is_command_allowed(command, &self.tool_context.command_allow_rules) {
                        // allowlist に一致 → 通常の context で実行
                        self.tool_context.clone()
                    } else {
                        // allowlist 不一致 → 承認を求める
                        match self.approver.approve_unsafe_shell(command) {
                            Approval::Approved => {
                                // 承認された → allow_unsafe=true の context で実行
                                self.tool_context.clone().with_allow_unsafe(true)
                            }
                            Approval::Denied => {
                                // 拒否された → ツールを実行せず ToolError として履歴に積む
                                let msg = "denied by user".to_string();
                                self.emit(&AgentEvent::ToolError {
                                    call_id: call_id.clone(),
                                    name: name.clone(),
                                    args: args.clone(),
                                    message: msg.clone(),
                                })?;
                                new_messages.push(Msg::tool_result(
                                    &call_id,
                                    &name,
                                    serde_json::json!({ "error": msg }),
                                ));
                                continue; // 次のツールへ
                            }
                        }
                    }
                } else {
                    // シェル系以外はそのまま実行
                    self.tool_context.clone()
                };

                match self.tool_registry.call(name.as_str(), args.clone(), &effective_ctx) {
                    Ok(result) => {
                        self.emit(&AgentEvent::ToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            args: args.clone(),
                            result: result.clone(),
                        })?;
                        new_messages.push(Msg::tool_result(&call_id, &name, result));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.emit(&AgentEvent::ToolError {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            args: args.clone(),
                            message: msg.clone(),
                        })?;
                        new_messages.push(Msg::tool_result(
                            &call_id,
                            &name,
                            serde_json::json!({ "error": msg }),
                        ));
                    }
                }
            }
            // ツール実行後は ExecutingTools のまま返し、run_until_done が再度 LLM を呼べるようにする
        }

        if run_state == RunState::Done {
            self.emit_end()?;
        }

        Ok((new_messages, run_state, assistant_text))
    }

    /// ツール実行後に再度 LLM を呼ぶループ。Done になるか max_turns に達するまで run_once を繰り返す。
    /// 上限到達時は `AgentLoopOutcome::ReachedLimit` を返し、呼び出し元で「続けますか？」等の判断ができる。
    pub fn run_until_done(
        &mut self,
        initial_messages: &[Msg],
        max_turns: usize,
    ) -> Result<AgentLoopOutcome, Error> {
        let mut messages = initial_messages.to_vec();
        let mut last_assistant_text = String::new();
        for _ in 0..max_turns {
            let (new_messages, state, assistant_text) = self.run_once(&messages)?;
            last_assistant_text = assistant_text;
            messages = new_messages;
            match state {
                RunState::Done => return Ok(AgentLoopOutcome::Done(messages, last_assistant_text)),
                RunState::ExecutingTools => continue,
                RunState::StreamingModel | RunState::Error => {
                    return Ok(AgentLoopOutcome::Done(messages, last_assistant_text));
                }
            }
        }
        Ok(AgentLoopOutcome::ReachedLimit(messages, last_assistant_text))
    }
}

/// テスト用: 固定の LlmEvent 列を返す Stub
#[cfg_attr(not(test), allow(dead_code))]
pub struct StubLlm {
    events: Vec<LlmEvent>,
}

#[allow(dead_code)] // テストで使用
impl StubLlm {
    pub fn new(events: Vec<LlmEvent>) -> Self {
        Self { events }
    }

    pub fn text_only(text: &str) -> Self {
        Self::new(vec![
            LlmEvent::TextDelta(text.to_string()),
            LlmEvent::Completed {
                finish: FinishReason::Stop,
            },
        ])
    }
}

impl LlmEventStream for StubLlm {
    fn stream_events(
        &self,
        _query: &str,
        _system_instruction: Option<&str>,
        _history: &[Message],
        _tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        for ev in &self.events {
            callback(ev.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::StubApproval;
    use common::llm::events::LlmEvent;
    use common::tool::{Tool, ToolError};

    /// テスト用: 何も出力しない EventSink
    struct StubEventSink;
    impl StubEventSink {
        fn new() -> Self {
            Self
        }
    }
    impl EventSink for StubEventSink {
        fn on_event(&mut self, _ev: &AgentEvent) -> Result<(), Error> {
            Ok(())
        }
        fn on_end(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    /// テスト用: name "run_shell" の Tool
    struct RunShellStubTool;
    impl RunShellStubTool {
        fn new() -> Self {
            Self
        }
    }
    impl Tool for RunShellStubTool {
        fn name(&self) -> &'static str {
            "run_shell"
        }
        fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
            let command = args.get("command").and_then(Value::as_str).unwrap_or("");
            Ok(serde_json::json!({
                "stdout": format!("{}\n", command),
                "stderr": "",
                "exit_code": 0
            }))
        }
    }

    #[test]
    fn test_msgs_to_provider_simple() {
        let msgs = vec![Msg::user("Hello")];
        let (sys, query, history) = msgs_to_provider(&msgs);
        assert!(sys.is_none());
        assert_eq!(query, "Hello");
        assert!(history.is_empty());
    }

    #[test]
    fn test_msgs_to_provider_with_history() {
        let msgs = vec![
            Msg::user("Hi"),
            Msg::assistant("Hello!"),
            Msg::user("Bye"),
        ];
        let (_sys, query, history) = msgs_to_provider(&msgs);
        assert_eq!(query, "Bye");
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_msgs_to_provider_with_tool_call_and_result() {
        let msgs = vec![
            Msg::user("run it"),
            Msg::assistant("ok"),
            Msg::ToolCall {
                call_id: "c1".to_string(),
                name: "run".to_string(),
                args: serde_json::json!({"cmd": "ls"}),
                thought_signature: Some("sig123".to_string()),
            },
            Msg::ToolResult {
                call_id: "c1".to_string(),
                name: "run".to_string(),
                result: serde_json::json!({"ok": true}),
            },
        ];
        let (_sys, query, history) = msgs_to_provider(&msgs);
        assert_eq!(query, "");
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
        assert!(history[1].tool_calls.is_some());
        assert_eq!(history[1].tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(history[1].tool_calls.as_ref().unwrap()[0].thought_signature.as_deref(), Some("sig123"));
        assert_eq!(history[2].role, "tool");
        assert!(history[2].tool_call_id.as_deref() == Some("c1"));
    }

    #[test]
    fn test_stub_llm_text_only() {
        let stub = StubLlm::text_only("hello");
        let mut received = Vec::new();
        stub.stream_events("q", None, &[], None, &mut |ev| {
            received.push(ev);
            Ok(())
        })
        .unwrap();
        assert_eq!(received.len(), 2);
        assert!(matches!(&received[0], LlmEvent::TextDelta(s) if s == "hello"));
        assert!(matches!(&received[1], LlmEvent::Completed { .. }));
    }

    #[test]
    fn test_agent_loop_run_once_text_only() {
        let stub = StubLlm::text_only("world");
        let registry = ToolRegistry::new();
        let ctx = ToolContext::new(None);
        let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
        let approver = Arc::new(StubApproval::approved());
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"));
        let messages = vec![Msg::user("Hi")];
        let (new_msgs, state, assistant_text) = loop_.run_once(&messages).unwrap();
        assert_eq!(state, RunState::Done);
        assert_eq!(assistant_text, "world");
        assert_eq!(new_msgs.len(), 2);
        assert!(matches!(&new_msgs[1], Msg::Assistant(s) if s == "world"));
    }

    #[test]
    fn test_agent_loop_run_once_with_tool_call() {
        let stub = StubLlm::new(vec![
            LlmEvent::ToolCallBegin {
                call_id: "c1".to_string(),
                name: "echo".to_string(),
                thought_signature: Some("test_signature".to_string()),
            },
            LlmEvent::ToolCallArgsDelta {
                call_id: "c1".to_string(),
                json_fragment: r#"{"message": "hello"}"#.to_string(),
            },
            LlmEvent::ToolCallEnd {
                call_id: "c1".to_string(),
            },
            LlmEvent::Completed {
                finish: FinishReason::ToolCalls,
            },
        ]);
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(common::tool::EchoTool::new()));
        let ctx = ToolContext::new(None);
        let sinks: Vec<Box<dyn EventSink>> = vec![];
        let approver = Arc::new(StubApproval::approved());
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"));
        let messages = vec![Msg::user("echo hello")];
        let (new_msgs, state, _text) = loop_.run_once(&messages).unwrap();

        assert_eq!(state, RunState::ExecutingTools);
        // user, (empty) assistant, tool_call, tool_result
        assert_eq!(new_msgs.len(), 4);
        assert!(matches!(&new_msgs[1], Msg::Assistant(s) if s.is_empty()));
        assert!(matches!(new_msgs[2], Msg::ToolCall { ref name, ref thought_signature, .. } if name == "echo" && thought_signature == &Some("test_signature".to_string())));
        assert!(matches!(new_msgs[3], Msg::ToolResult { ref name, .. } if name == "echo"));
    }

    #[test]
    fn test_agent_loop_shell_tool_denied() {
        // allowlist 不一致かつ Denied の場合、ツールは実行されない
        let stub = StubLlm::new(vec![
            LlmEvent::ToolCallBegin {
                call_id: "c1".to_string(),
                name: "run_shell".to_string(),
                thought_signature: None,
            },
            LlmEvent::ToolCallArgsDelta {
                call_id: "c1".to_string(),
                json_fragment: r#"{"command": "rm -rf /"}"#.to_string(),
            },
            LlmEvent::ToolCallEnd {
                call_id: "c1".to_string(),
            },
            LlmEvent::Completed {
                finish: FinishReason::ToolCalls,
            },
        ]);
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(RunShellStubTool::new()));
        let ctx = ToolContext::new(None); // allowlist 空
        let sinks: Vec<Box<dyn EventSink>> = vec![];
        let approver = Arc::new(StubApproval::denied());
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"));
        let messages = vec![Msg::user("run it")];
        let (new_msgs, state, _text) = loop_.run_once(&messages).unwrap();

        assert_eq!(state, RunState::ExecutingTools);
        // user, (empty) assistant, tool_call, tool_result (error)
        assert_eq!(new_msgs.len(), 4);
        // ツール結果はエラー
        if let Msg::ToolResult { result, .. } = &new_msgs[3] {
            assert!(result.get("error").is_some());
            assert!(result["error"].as_str().unwrap().contains("denied"));
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[test]
    fn test_agent_loop_shell_tool_approved() {
        // allowlist 不一致でも Approved の場合、ツールは実行される
        let stub = StubLlm::new(vec![
            LlmEvent::ToolCallBegin {
                call_id: "c1".to_string(),
                name: "run_shell".to_string(),
                thought_signature: None,
            },
            LlmEvent::ToolCallArgsDelta {
                call_id: "c1".to_string(),
                json_fragment: r#"{"command": "echo approved"}"#.to_string(),
            },
            LlmEvent::ToolCallEnd {
                call_id: "c1".to_string(),
            },
            LlmEvent::Completed {
                finish: FinishReason::ToolCalls,
            },
        ]);
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(RunShellStubTool::new()));
        let ctx = ToolContext::new(None); // allowlist 空
        let sinks: Vec<Box<dyn EventSink>> = vec![];
        let approver = Arc::new(StubApproval::approved());
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"));
        let messages = vec![Msg::user("run it")];
        let (new_msgs, state, _text) = loop_.run_once(&messages).unwrap();

        assert_eq!(state, RunState::ExecutingTools);
        // user, (empty) assistant, tool_call, tool_result (success)
        assert_eq!(new_msgs.len(), 4);
        // ツール結果は成功（Stub は command を stdout に echo する）
        if let Msg::ToolResult { result, .. } = &new_msgs[3] {
            assert!(result.get("stdout").is_some());
            assert_eq!(result["stdout"].as_str(), Some("echo approved\n"));
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[test]
    fn test_agent_loop_run_until_done_reached_limit() {
        // Stub は毎回 ToolCalls で終わるので Done にならない。max_turns で打ち切られ ReachedLimit になる。
        let stub = StubLlm::new(vec![
            LlmEvent::ToolCallBegin {
                call_id: "c1".to_string(),
                name: "echo".to_string(),
                thought_signature: None,
            },
            LlmEvent::ToolCallArgsDelta {
                call_id: "c1".to_string(),
                json_fragment: r#"{"message": "hi"}"#.to_string(),
            },
            LlmEvent::ToolCallEnd {
                call_id: "c1".to_string(),
            },
            LlmEvent::Completed {
                finish: FinishReason::ToolCalls,
            },
        ]);
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(common::tool::EchoTool::new()));
        let ctx = ToolContext::new(None);
        let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
        let approver = Arc::new(StubApproval::approved());
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"));
        let messages = vec![Msg::user("echo")];
        let outcome = loop_.run_until_done(&messages, 2).unwrap();
        match &outcome {
            AgentLoopOutcome::ReachedLimit(msgs, _) => {
                assert!(!msgs.is_empty());
            }
            AgentLoopOutcome::Done(_, _) => panic!("expected ReachedLimit"),
        }
    }

    #[test]
    fn test_agent_loop_run_until_done_done() {
        let stub = StubLlm::text_only("bye");
        let registry = ToolRegistry::new();
        let ctx = ToolContext::new(None);
        let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
        let approver = Arc::new(StubApproval::approved());
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"));
        let messages = vec![Msg::user("Hi")];
        let outcome = loop_.run_until_done(&messages, 16).unwrap();
        match &outcome {
            AgentLoopOutcome::Done(msgs, text) => {
                assert_eq!(msgs.len(), 2);
                assert_eq!(text.as_str(), "bye");
            }
            AgentLoopOutcome::ReachedLimit(_, _) => panic!("expected Done"),
        }
    }
}
