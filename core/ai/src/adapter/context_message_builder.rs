//! 履歴＋クエリから Vec<Msg> を構築する標準アダプタ
//!
//! Phase B で HistoryReducer + ContextBudget を組み込む。

use crate::domain::Query;
use crate::ports::outbound::{ContextMessageBuilder, QueryPlacement};
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use std::sync::Arc;

use crate::domain::{ContextBudget, HistoryReducer};

/// 履歴を LlmMessage のスライスから Msg 列に変換する（system は呼び出し側で先頭に付与）
fn history_to_msgs(messages: &[LlmMessage]) -> Vec<Msg> {
    let mut msgs = Vec::with_capacity(messages.len());
    for m in messages {
        if m.role == "user" {
            msgs.push(Msg::user(&m.content));
        } else if m.role == "tool" {
            if let Some(ref call_id) = m.tool_call_id {
                let name = m.tool_name.as_deref().unwrap_or("");
                msgs.push(Msg::tool_result(
                    call_id,
                    name,
                    serde_json::from_str(&m.content).unwrap_or(serde_json::json!({})),
                ));
            }
        } else {
            msgs.push(Msg::assistant(&m.content));
            if let Some(ref tool_calls) = m.tool_calls {
                for tc in tool_calls {
                    msgs.push(Msg::tool_call(
                        &tc.id,
                        &tc.name,
                        tc.args.clone(),
                        tc.thought_signature.clone(),
                    ));
                }
            }
        }
    }
    msgs
}

/// 標準の ContextMessageBuilder（Reducer + Budget 対応）
pub struct StdContextMessageBuilder {
    reducer: Arc<dyn HistoryReducer>,
    budget: ContextBudget,
}

impl StdContextMessageBuilder {
    pub fn new(reducer: Arc<dyn HistoryReducer>, budget: ContextBudget) -> Self {
        Self { reducer, budget }
    }
}

impl ContextMessageBuilder for StdContextMessageBuilder {
    fn build(
        &self,
        history: &[LlmMessage],
        query: Option<&Query>,
        system_instruction: Option<&str>,
        query_placement: QueryPlacement,
    ) -> Vec<Msg> {
        // 1) AppendAtEnd のときだけ末尾に query を仮の user message として足した all_messages を作る
        let all_messages: Vec<LlmMessage> = if query_placement == QueryPlacement::AppendAtEnd {
            if let Some(q) = query {
                let mut v = history.to_vec();
                v.push(LlmMessage::user(q.as_ref()));
                v
            } else {
                history.to_vec()
            }
        } else {
            history.to_vec()
        };

        // 2) reducer で縮約
        let reduced = self.reducer.reduce(&all_messages, self.budget);

        // 3) Msg に変換し、system_instruction を先頭に付与
        let mut msgs = Vec::new();
        if let Some(s) = system_instruction {
            msgs.push(Msg::system(s));
        }
        msgs.extend(history_to_msgs(&reduced));
        msgs
    }
}
