//! ツール付きエージェントループの 1 ラウンド実行。

use std::str::FromStr;
use std::sync::Arc;

use crate::application::tool_defs::definitions_for;
use crate::application::tool_round::rejected::rejected_tool_result;
use crate::domain::{
    is_known_tool, ChatMessage, ExecutedToolCall, ToolApprovalState, ToolName, ToolRiskClass,
    GIT_DIFF, GIT_STATUS, GREP, LIST_DIR, READ_FILE, SHELL_EXEC,
};
use crate::ports::outbound::{
    LlmError, LlmProvider, ToolExecutionContext, ToolRegistry, ToolsConfig,
};

/// 1 ラウンドの結果。max-round 終端は `AgentTurnService` + terminator が担当。
#[derive(Debug, Clone)]
pub enum RoundOutcome {
    Completed {
        assistant: ChatMessage,
        executed: Vec<ExecutedToolCall>,
    },
    Continue {
        conversation: Vec<ChatMessage>,
        executed: Vec<ExecutedToolCall>,
    },
}

pub struct ToolRoundExecutor {
    llm: Arc<dyn LlmProvider>,
    registry: Arc<dyn ToolRegistry>,
    tools_config: ToolsConfig,
}

fn classify_tool(name: &str) -> (ToolRiskClass, ToolApprovalState) {
    match name {
        SHELL_EXEC => (
            ToolRiskClass::DangerousShell,
            ToolApprovalState::ExplicitClientOptIn,
        ),
        READ_FILE | LIST_DIR | GREP | GIT_DIFF | GIT_STATUS => {
            (ToolRiskClass::ReadOnly, ToolApprovalState::NotRequired)
        }
        _ => (
            ToolRiskClass::WriteLike,
            ToolApprovalState::ExplicitClientOptIn,
        ),
    }
}

impl ToolRoundExecutor {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        registry: Arc<dyn ToolRegistry>,
        tools_config: ToolsConfig,
    ) -> Self {
        Self {
            llm,
            registry,
            tools_config,
        }
    }

    pub fn tools_config(&self) -> &ToolsConfig {
        &self.tools_config
    }

    /// 1 回: LLM（tools 付き）→ tool 実行 → conversation 更新。
    pub async fn run_one_round(
        &self,
        conversation: &[ChatMessage],
        allowed_tools: &[ToolName],
        tool_ctx: &ToolExecutionContext,
        executed_so_far: &[ExecutedToolCall],
    ) -> Result<RoundOutcome, LlmError> {
        let tool_defs = definitions_for(allowed_tools);
        let step = self
            .llm
            .complete_with_tools(conversation, &tool_defs)
            .await?;

        let mut executed = executed_so_far.to_vec();

        if step.tool_calls.is_empty() {
            return Ok(RoundOutcome::Completed {
                assistant: step.assistant,
                executed,
            });
        }

        let mut next_conversation = conversation.to_vec();
        next_conversation.push(step.assistant.clone());

        for tc in &step.tool_calls {
            let (record, result) = if !is_known_tool(&tc.name) {
                rejected_tool_result(
                    tc,
                    "tool_not_implemented",
                    format!("unknown tool: {}", tc.name),
                )
            } else {
                let name =
                    ToolName::from_str(&tc.name).expect("is_known_tool implies valid ToolName");
                if !allowed_tools.contains(&name) {
                    rejected_tool_result(
                        tc,
                        "tool_not_allowed",
                        format!("model requested disallowed tool: {}", tc.name),
                    )
                } else if let Some(executor) = self.registry.get(&name) {
                    executor
                        .execute(
                            &tc.id,
                            &tc.arguments,
                            self.tools_config.exec_timeout_ms,
                            tool_ctx,
                        )
                        .await
                } else {
                    rejected_tool_result(
                        tc,
                        "tool_not_implemented",
                        format!("tool not implemented: {}", tc.name),
                    )
                }
            };
            let record = if record.risk_class.is_some() {
                record
            } else {
                let (risk, approval) = classify_tool(&record.name);
                record.with_audit(risk, approval, false)
            };
            executed.push(record);
            let content = if result.is_error {
                format!("[tool error]\n{}", result.content)
            } else {
                result.content
            };
            next_conversation.push(ChatMessage::tool(tc.id.clone(), content));
        }

        Ok(RoundOutcome::Continue {
            conversation: next_conversation,
            executed,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde_json::{json, Value};

    use super::*;
    use crate::domain::{
        LlmStepResult, MessageRole, ToolCall, ToolName, ToolResult, READ_FILE, SHELL_EXEC,
    };
    use crate::ports::outbound::{ToolDefinition, ToolExecutor};

    struct StepLlm {
        steps: Mutex<Vec<LlmStepResult>>,
    }

    impl StepLlm {
        fn new(steps: Vec<LlmStepResult>) -> Arc<Self> {
            Arc::new(Self {
                steps: Mutex::new(steps),
            })
        }
    }

    #[async_trait]
    impl LlmProvider for StepLlm {
        async fn complete(&self, _messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
            Err(LlmError::Provider("unexpected complete".into()))
        }

        async fn complete_with_tools(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<LlmStepResult, LlmError> {
            let mut guard = self
                .steps
                .lock()
                .map_err(|e| LlmError::Provider(e.to_string()))?;
            guard
                .pop()
                .ok_or_else(|| LlmError::Provider("no more steps".into()))
        }
    }

    struct StubTool {
        name: ToolName,
        output: String,
    }

    #[async_trait]
    impl ToolExecutor for StubTool {
        fn name(&self) -> ToolName {
            self.name.clone()
        }

        async fn execute(
            &self,
            tool_call_id: &str,
            arguments: &Value,
            _timeout_ms: u64,
            _ctx: &ToolExecutionContext,
        ) -> (ExecutedToolCall, ToolResult) {
            (
                ExecutedToolCall::ok(
                    tool_call_id.into(),
                    self.name.clone(),
                    arguments.clone(),
                    self.output.clone(),
                ),
                ToolResult {
                    tool_call_id: tool_call_id.into(),
                    content: self.output.clone(),
                    is_error: false,
                },
            )
        }
    }

    struct MapRegistry {
        tools: HashMap<ToolName, Arc<dyn ToolExecutor>>,
    }

    impl MapRegistry {
        fn new(tools: Vec<Arc<dyn ToolExecutor>>) -> Arc<Self> {
            let mut map = HashMap::new();
            for tool in tools {
                map.insert(tool.name(), tool);
            }
            Arc::new(Self { tools: map })
        }
    }

    impl ToolRegistry for MapRegistry {
        fn get(&self, name: &ToolName) -> Option<Arc<dyn ToolExecutor>> {
            self.tools.get(name).cloned()
        }
    }

    fn executor(llm: Arc<dyn LlmProvider>, registry: Arc<dyn ToolRegistry>) -> ToolRoundExecutor {
        ToolRoundExecutor::new(llm, registry, ToolsConfig::default())
    }

    fn tool_ctx() -> ToolExecutionContext {
        ToolExecutionContext::new(crate::domain::ClientCwd::parse("/tmp/proj").expect("cwd"))
    }

    #[tokio::test]
    async fn completed_when_model_returns_no_tools() {
        let llm = StepLlm::new(vec![LlmStepResult::text_only("done")]);
        let exec = executor(llm, MapRegistry::new(vec![]));
        let outcome = exec
            .run_one_round(
                &[ChatMessage::user("hi")],
                &[ToolName::read_file()],
                &tool_ctx(),
                &[],
            )
            .await
            .expect("round");

        match outcome {
            RoundOutcome::Completed {
                assistant,
                executed,
            } => {
                assert_eq!(assistant.content, "done");
                assert!(executed.is_empty());
            }
            RoundOutcome::Continue { .. } => panic!("expected Completed"),
        }
    }

    #[tokio::test]
    async fn continue_after_one_tool_execution() {
        let llm = StepLlm::new(vec![LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: READ_FILE.to_string(),
                arguments: json!({"path": "a.md"}),
                provider_extras: None,
            }],
        )]);
        let registry = MapRegistry::new(vec![Arc::new(StubTool {
            name: ToolName::read_file(),
            output: "file body".into(),
        })]);
        let exec = executor(llm, registry);
        let outcome = exec
            .run_one_round(
                &[ChatMessage::user("read")],
                &[ToolName::read_file()],
                &tool_ctx(),
                &[],
            )
            .await
            .expect("round");

        match outcome {
            RoundOutcome::Continue {
                conversation,
                executed,
            } => {
                assert_eq!(conversation.len(), 3);
                assert_eq!(conversation[2].role, MessageRole::Tool);
                assert_eq!(conversation[2].content, "file body");
                assert_eq!(executed.len(), 1);
                assert_eq!(executed[0].output.as_deref(), Some("file body"));
            }
            RoundOutcome::Completed { .. } => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn continue_on_disallowed_tool() {
        let llm = StepLlm::new(vec![LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: SHELL_EXEC.to_string(),
                arguments: json!({"command": "ls"}),
                provider_extras: None,
            }],
        )]);
        let exec = executor(llm, MapRegistry::new(vec![]));
        let outcome = exec
            .run_one_round(
                &[ChatMessage::user("run")],
                &[ToolName::read_file()],
                &tool_ctx(),
                &[],
            )
            .await
            .expect("round");

        match outcome {
            RoundOutcome::Continue {
                conversation,
                executed,
            } => {
                assert!(
                    conversation[2].content.contains("tool_not_allowed")
                        || conversation[2].content.contains("disallowed")
                );
                assert_eq!(executed.len(), 1);
                assert_eq!(executed[0].error.as_deref(), Some("tool_not_allowed"));
            }
            RoundOutcome::Completed { .. } => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn continue_on_unknown_tool_name_from_model() {
        let llm = StepLlm::new(vec![LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "dir_list".to_string(),
                arguments: json!({}),
                provider_extras: None,
            }],
        )]);
        let exec = executor(llm, MapRegistry::new(vec![]));
        let outcome = exec
            .run_one_round(
                &[ChatMessage::user("list")],
                &[ToolName::read_file()],
                &tool_ctx(),
                &[],
            )
            .await
            .expect("round");

        match outcome {
            RoundOutcome::Continue {
                conversation,
                executed,
            } => {
                assert!(conversation[2].content.contains("unknown tool: dir_list"));
                assert_eq!(executed.len(), 1);
                assert_eq!(executed[0].error.as_deref(), Some("tool_not_implemented"));
                assert_eq!(executed[0].name, "dir_list");
            }
            RoundOutcome::Completed { .. } => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn continue_on_unimplemented_tool() {
        let llm = StepLlm::new(vec![LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: READ_FILE.to_string(),
                arguments: json!({"path": "a.md"}),
                provider_extras: None,
            }],
        )]);
        let exec = executor(llm, MapRegistry::new(vec![]));
        let outcome = exec
            .run_one_round(
                &[ChatMessage::user("read")],
                &[ToolName::read_file()],
                &tool_ctx(),
                &[],
            )
            .await
            .expect("round");

        match outcome {
            RoundOutcome::Continue {
                conversation,
                executed,
            } => {
                assert!(conversation[2].content.contains("not implemented"));
                assert_eq!(executed.len(), 1);
                assert_eq!(executed[0].error.as_deref(), Some("tool_not_implemented"));
            }
            RoundOutcome::Completed { .. } => panic!("expected Continue"),
        }
    }

    #[tokio::test]
    async fn multiple_tools_preserve_order() {
        let llm = StepLlm::new(vec![LlmStepResult::with_tool_calls(
            "",
            vec![
                ToolCall {
                    id: "c1".into(),
                    name: READ_FILE.to_string(),
                    arguments: json!({"path": "a.md"}),
                    provider_extras: None,
                },
                ToolCall {
                    id: "c2".into(),
                    name: READ_FILE.to_string(),
                    arguments: json!({"path": "b.md"}),
                    provider_extras: None,
                },
            ],
        )]);
        let registry = MapRegistry::new(vec![Arc::new(StubTool {
            name: ToolName::read_file(),
            output: "same".into(),
        })]);
        let exec = executor(llm, registry);
        let prior = vec![ExecutedToolCall::ok(
            "prev".into(),
            ToolName::read_file(),
            json!({}),
            "old".into(),
        )];
        let outcome = exec
            .run_one_round(
                &[ChatMessage::user("read both")],
                &[ToolName::read_file()],
                &tool_ctx(),
                &prior,
            )
            .await
            .expect("round");

        match outcome {
            RoundOutcome::Continue {
                conversation,
                executed,
            } => {
                assert_eq!(executed.len(), 3);
                assert_eq!(executed[0].id, "prev");
                assert_eq!(executed[1].id, "c1");
                assert_eq!(executed[2].id, "c2");
                assert_eq!(conversation.len(), 4);
                assert_eq!(conversation[1].role, MessageRole::Assistant);
                assert_eq!(conversation[2].role, MessageRole::Tool);
                assert_eq!(conversation[3].role, MessageRole::Tool);
            }
            RoundOutcome::Completed { .. } => panic!("expected Continue"),
        }
    }
}
