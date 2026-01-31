//! AgentLoop: イベント解釈器 + 状態機械
//!
//! 直列の transaction script をやめ、RunState で遷移する。
//! LLM から ToolCallEnd が来たら tool 実行フェーズへ遷移し、結果を messages に注入する。

use common::error::Error;
use common::llm::events::{FinishReason, LlmEvent};
use common::llm::provider::Message;
use common::msg::Msg;
use common::sink::{AgentEvent, EventSink};
use common::tool::{ToolContext, ToolRegistry};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

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

/// LLM ストリームを LlmEvent 列で受け取るポート（テストでは StubLlm で差し替え）
pub trait LlmEventStream: Send {
    fn stream_events(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error>;
}

/// Vec<Msg> をドライバ用 (system_instruction, query, history) に変換
/// ToolCall/ToolResult は Assistant(content, tool_calls) と Tool(call_id, result) に変換
pub fn msgs_to_provider(msgs: &[Msg]) -> (Option<String>, String, Vec<Message>) {
    let mut system: Option<String> = None;
    let mut list: Vec<Message> = Vec::new();
    let mut last_user: Option<String> = None;
    let mut pending_assistant: Option<String> = None;
    let mut pending_tool_calls: Vec<(String, String, Value)> = Vec::new();

    fn flush_assistant_with_tool_calls(
        list: &mut Vec<Message>,
        pending_assistant: &mut Option<String>,
        pending_tool_calls: &mut Vec<(String, String, Value)>,
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
            Msg::ToolCall { call_id, name, args } => {
                if pending_assistant.is_none() {
                    pending_assistant = Some(String::new());
                }
                pending_tool_calls.push((call_id.clone(), name.clone(), args.clone()));
            }
            Msg::ToolResult { call_id, result, .. } => {
                flush_assistant_with_tool_calls(&mut list, &mut pending_assistant, &mut pending_tool_calls);
                let content = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                list.push(Message::tool_result(call_id.clone(), content));
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

/// ストリーム中に蓄積するツール呼び出し（call_id -> (name, args_json_fragments)）
#[allow(dead_code)] // completed は将来の複数ツール蓄積用
struct ToolCallAccumulator {
    current_id: Option<String>,
    current_name: Option<String>,
    args_fragments: String,
    completed: Vec<(String, String, Value)>,
}

impl ToolCallAccumulator {
    fn new() -> Self {
        Self {
            current_id: None,
            current_name: None,
            args_fragments: String::new(),
            completed: Vec::new(),
        }
    }

    fn on_begin(&mut self, call_id: String, name: String) {
        self.current_id = Some(call_id);
        self.current_name = Some(name);
        self.args_fragments.clear();
    }

    fn on_args_delta(&mut self, fragment: String) {
        self.args_fragments.push_str(&fragment);
    }

    fn on_end(&mut self, call_id: String) -> Result<Option<(String, String, Value)>, Error> {
        let name = self.current_name.take().unwrap_or_default();
        self.current_id = None;
        let args = if self.args_fragments.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&self.args_fragments)
                .map_err(|e| Error::json(format!("Invalid tool args JSON: {}", e)))?
        };
        self.args_fragments.clear();
        Ok(Some((call_id, name, args)))
    }
}

/// AgentLoop: 状態機械で LlmEvent を処理し、Sink に流す
pub struct AgentLoop<S: LlmEventStream> {
    stream: S,
    tool_registry: ToolRegistry,
    tool_context: ToolContext,
    sinks: Vec<Box<dyn EventSink>>,
}

impl<S: LlmEventStream> AgentLoop<S> {
    pub fn new(
        stream: S,
        tool_registry: ToolRegistry,
        tool_context: ToolContext,
        sinks: Vec<Box<dyn EventSink>>,
    ) -> Self {
        Self {
            stream,
            tool_registry,
            tool_context,
            sinks,
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
            .stream_events(&query, system_instruction, &history, &mut cb)?;

        let mut assistant_text = String::new();
        let mut accumulator = ToolCallAccumulator::new();
        let mut pending_tool_calls: Vec<(String, String, Value)> = Vec::new();
        let mut run_state = RunState::StreamingModel;

        for ev in collected.borrow().iter() {
            match ev {
                LlmEvent::TextDelta(s) => assistant_text.push_str(s),
                LlmEvent::ToolCallBegin { call_id, name } => {
                    accumulator.on_begin(call_id.clone(), name.clone());
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
        if !assistant_text.is_empty() {
            new_messages.push(Msg::assistant(assistant_text.clone()));
        }

        if run_state == RunState::ExecutingTools && !pending_tool_calls.is_empty() {
            for (call_id, name, args) in pending_tool_calls {
                match self.tool_registry.call(name.as_str(), args.clone(), &self.tool_context) {
                    Ok(result) => {
                        self.emit(&AgentEvent::ToolResult {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            result: result.clone(),
                        })?;
                        new_messages.push(Msg::tool_result(&call_id, &name, result));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.emit(&AgentEvent::ToolError {
                            call_id: call_id.clone(),
                            name: name.clone(),
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
    pub fn run_until_done(
        &mut self,
        initial_messages: &[Msg],
        max_turns: usize,
    ) -> Result<(Vec<Msg>, String), Error> {
        let mut messages = initial_messages.to_vec();
        let mut last_assistant_text = String::new();
        for _ in 0..max_turns {
            let (new_messages, state, assistant_text) = self.run_once(&messages)?;
            last_assistant_text = assistant_text;
            messages = new_messages;
            match state {
                RunState::Done => return Ok((messages, last_assistant_text)),
                RunState::ExecutingTools => continue,
                RunState::StreamingModel | RunState::Error => {
                    return Ok((messages, last_assistant_text));
                }
            }
        }
        Ok((messages, last_assistant_text))
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
    use common::llm::events::LlmEvent;

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
        assert_eq!(history[2].role, "tool");
        assert!(history[2].tool_call_id.as_deref() == Some("c1"));
    }

    #[test]
    fn test_stub_llm_text_only() {
        let stub = StubLlm::text_only("hello");
        let mut received = Vec::new();
        stub.stream_events("q", None, &[], &mut |ev| {
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
        let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(crate::adapter::StdoutSink::new())];
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks);
        let messages = vec![Msg::user("Hi")];
        let (new_msgs, state, assistant_text) = loop_.run_once(&messages).unwrap();
        assert_eq!(state, RunState::Done);
        assert_eq!(assistant_text, "world");
        assert_eq!(new_msgs.len(), 2);
        assert!(matches!(&new_msgs[1], Msg::Assistant(s) if s == "world"));
    }

    #[test]
    fn test_agent_loop_run_until_done_one_turn() {
        let stub = StubLlm::text_only("done");
        let registry = ToolRegistry::new();
        let ctx = ToolContext::new(None);
        let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(crate::adapter::StdoutSink::new())];
        let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks);
        let messages = vec![Msg::user("Hi")];
        let (final_msgs, assistant_text) = loop_.run_until_done(&messages, 4).unwrap();
        assert_eq!(assistant_text, "done");
        assert_eq!(final_msgs.len(), 2);
        assert!(matches!(&final_msgs[1], Msg::Assistant(s) if s == "done"));
    }
}
