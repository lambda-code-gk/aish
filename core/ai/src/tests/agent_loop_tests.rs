//! AgentLoop の単体テスト（StubLlm は adapter のテスト用実装を使用）

use std::sync::Arc;

use common::llm::events::{FinishReason, LlmEvent};
use common::msg::Msg;
use common::sink::{AgentEvent, EventSink};
use common::tool::{Tool, ToolContext, ToolError, ToolRegistry};
use serde_json::Value;

use crate::adapter::stub_llm::StubLlm;
use crate::domain::approval::StubApproval;
use crate::ports::outbound::LlmEventStream;
use crate::usecase::agent_loop::{
    count_tool_results, msgs_to_provider, AgentLoop, AgentLoopOutcome, RunState,
};

/// テスト用: 何も出力しない EventSink
struct StubEventSink;
impl StubEventSink {
    fn new() -> Self {
        Self
    }
}
impl EventSink for StubEventSink {
    fn on_event(&mut self, _ev: &AgentEvent) -> Result<(), common::error::Error> {
        Ok(())
    }
    fn on_end(&mut self) -> Result<(), common::error::Error> {
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
    let stub = Arc::new(StubLlm::text_only("world"));
    let registry = ToolRegistry::new();
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
    let approver = Arc::new(StubApproval::approved());
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"), None);
    let messages = vec![Msg::user("Hi")];
    let (new_msgs, state, assistant_text) = loop_.run_once(&messages, None).unwrap();
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
    let stub = Arc::new(stub);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"), None);
    let messages = vec![Msg::user("echo hello")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    assert_eq!(new_msgs.len(), 4);
    assert!(matches!(&new_msgs[1], Msg::Assistant(s) if s.is_empty()));
    assert!(matches!(new_msgs[2], Msg::ToolCall { ref name, ref thought_signature, .. } if name == "echo" && thought_signature == &Some("test_signature".to_string())));
    assert!(matches!(new_msgs[3], Msg::ToolResult { ref name, .. } if name == "echo"));
}

#[test]
fn test_agent_loop_shell_tool_denied() {
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
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![];
    let approver = Arc::new(StubApproval::denied());
    let stub = Arc::new(stub);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"), None);
    let messages = vec![Msg::user("run it")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    assert_eq!(new_msgs.len(), 4);
    if let Msg::ToolResult { result, .. } = &new_msgs[3] {
        assert!(result.get("error").is_some());
        assert!(result["error"].as_str().unwrap().contains("denied"));
    } else {
        panic!("Expected ToolResult");
    }
}

#[test]
fn test_agent_loop_shell_tool_approved() {
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
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![];
    let approver = Arc::new(StubApproval::approved());
    let stub = Arc::new(stub);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"), None);
    let messages = vec![Msg::user("run it")];
    let (new_msgs, state, _text) = loop_.run_once(&messages, None).unwrap();

    assert_eq!(state, RunState::ExecutingTools);
    assert_eq!(new_msgs.len(), 4);
    if let Msg::ToolResult { result, .. } = &new_msgs[3] {
        assert!(result.get("stdout").is_some());
        assert_eq!(result["stdout"].as_str(), Some("echo approved\n"));
    } else {
        panic!("Expected ToolResult");
    }
}

#[test]
fn test_agent_loop_run_until_done_reached_limit() {
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
    let stub = Arc::new(stub);
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"), None);
    let messages = vec![Msg::user("echo")];
    let outcome = loop_.run_until_done(&messages, 2, 100).unwrap();
    match &outcome {
        AgentLoopOutcome::ReachedLimit(msgs, _) => {
            assert!(!msgs.is_empty());
        }
        AgentLoopOutcome::Done(_, _) => panic!("expected ReachedLimit"),
    }
}

#[test]
fn test_agent_loop_run_until_done_done() {
    let stub = Arc::new(StubLlm::text_only("bye"));
    let registry = ToolRegistry::new();
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
    let approver = Arc::new(StubApproval::approved());
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"), None);
    let messages = vec![Msg::user("Hi")];
    let outcome = loop_.run_until_done(&messages, 16, 16).unwrap();
    match &outcome {
        AgentLoopOutcome::Done(msgs, text) => {
            assert_eq!(msgs.len(), 2);
            assert_eq!(text.as_str(), "bye");
        }
        AgentLoopOutcome::ReachedLimit(_, _) => panic!("expected Done"),
    }
}

#[test]
fn test_agent_loop_run_until_done_capped_by_tool_calls() {
    let events: Vec<LlmEvent> = (1..=5)
        .flat_map(|i| {
            let call_id = format!("c{}", i);
            vec![
                LlmEvent::ToolCallBegin {
                    call_id: call_id.clone(),
                    name: "run_shell".to_string(),
                    thought_signature: None,
                },
                LlmEvent::ToolCallArgsDelta {
                    call_id: call_id.clone(),
                    json_fragment: format!(r#"{{"command": "echo {}"}}"#, i),
                },
                LlmEvent::ToolCallEnd { call_id },
            ]
        })
        .chain(std::iter::once(LlmEvent::Completed {
            finish: FinishReason::ToolCalls,
        }))
        .collect();
    let stub = Arc::new(StubLlm::new(events));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(RunShellStubTool::new()));
    let ctx = ToolContext::new(None);
    let sinks: Vec<Box<dyn EventSink>> = vec![Box::new(StubEventSink::new())];
    let approver = Arc::new(StubApproval::approved());
    let mut loop_ = AgentLoop::new(stub, registry, ctx, sinks, approver, Some("run_shell"), None);
    let messages = vec![Msg::user("echo many")];
    let outcome = loop_.run_until_done(&messages, 10, 3).unwrap();
    match &outcome {
        AgentLoopOutcome::ReachedLimit(msgs, _) => {
            assert_eq!(
                count_tool_results(msgs),
                3,
                "max_tool_calls=3 なので実行は3件まで"
            );
        }
        AgentLoopOutcome::Done(_, _) => panic!("expected ReachedLimit (tool call cap)"),
    }
}
