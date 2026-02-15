//! クエリ終了時にセッション履歴から知見を抽出しメモリに保存するハンドラ

use crate::adapter::lifecycle::LifecycleHandler;
use crate::adapter::memory_storage;
use crate::domain::{LifecycleEvent, MemoryEntry};
use common::error::Error;
use common::msg::Msg;
use common::ports::outbound::{now_iso8601, Log};
use serde_json::Value;
use std::sync::Arc;

use crate::ports::outbound::LlmCompletion;

const EXTRACTION_SYSTEM: &str = r#"You are a knowledge extraction agent. \
Analyze the following terminal session history and extract any useful knowledge that could be reused in the future. \
Focus on:
- Successful solutions to specific errors.
- Non-trivial code patterns or shell commands.
- Project-specific configuration or workflow details.
- Best practices discovered during the session.

If there is no significant knowledge to extract (e.g., only trivial file listing or simple questions), respond with 'NONE'. \
Otherwise, respond ONLY with a JSON object in the following format:
{
  "content": "A clear and concise summary of the knowledge",
  "category": "One of: code_pattern, error_solution, workflow, best_practice, configuration, general",
  "keywords": ["keyword1", "keyword2", ...],
  "subject": "A brief subject or title describing what this knowledge is about"
}"#;

const RESULT_TRUNCATE: usize = 500;

/// クエリ終了時にセッション履歴を LLM で要約し、知見があれば永続メモリに保存する
pub struct SelfImproveHandler {
    llm: Arc<dyn LlmCompletion>,
    log: Arc<dyn Log>,
}

impl SelfImproveHandler {
    pub fn new(llm: Arc<dyn LlmCompletion>, log: Arc<dyn Log>) -> Self {
        Self { llm, log }
    }
}

impl LifecycleHandler for SelfImproveHandler {
    fn on_event(&self, event: &LifecycleEvent) -> Result<(), Error> {
        let (memory_dir_project, memory_dir_global, messages) = match event {
            LifecycleEvent::QueryEnd {
                memory_dir_project,
                memory_dir_global,
                messages,
                ..
            } => (memory_dir_project, memory_dir_global, messages),
        };
        let summary = msgs_to_session_summary(messages);
        if summary.trim().is_empty() {
            return Ok(());
        }
        let user_msg = format!("Session history:\n{}", summary);
        let response = match self.llm.complete(Some(EXTRACTION_SYSTEM), &user_msg) {
            Ok(s) => s,
            Err(e) => {
                // 自己改善の失敗はクエリ成功を妨げない
                return Err(e);
            }
        };
        let response = response.trim();
        if response.is_empty()
            || response.eq_ignore_ascii_case("NONE")
            || response == "null"
        {
            return Ok(());
        }
        let json_str = extract_json_from_response(response);
        let Some(obj) = parse_extraction_json(&json_str) else {
            return Ok(());
        };
        let content = match obj.get("content").and_then(Value::as_str) {
            Some(s) if !s.trim().is_empty() => s.to_string(),
            _ => return Ok(()),
        };
        let category = obj
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("general")
            .to_string();
        let keywords: Vec<String> = obj
            .get("keywords")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let subject = obj
            .get("subject")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let dir = memory_dir_project
            .as_deref()
            .unwrap_or(memory_dir_global.as_path());
        let timestamp = now_iso8601();
        let entry = MemoryEntry::new("", content, category, keywords, subject, timestamp);
        let _ = memory_storage::save_entry(dir, &entry, Some(self.log.as_ref()))?;
        Ok(())
    }
}

fn msgs_to_session_summary(msgs: &[Msg]) -> String {
    let mut out = String::new();
    for m in msgs {
        match m {
            Msg::User(s) => {
                out.push_str("USER: ");
                out.push_str(s);
                out.push('\n');
            }
            Msg::Assistant(s) => {
                out.push_str("ASSISTANT: ");
                out.push_str(s);
                out.push('\n');
            }
            Msg::ToolCall { name, args, .. } => {
                let args_str = args.to_string();
                out.push_str("TOOLS: ");
                out.push_str(name);
                out.push('(');
                out.push_str(&args_str);
                out.push_str(")\n");
            }
            Msg::ToolResult { result, .. } => {
                let snippet = tool_result_snippet(result);
                out.push_str("RESULT: ");
                out.push_str(&snippet);
                out.push('\n');
            }
            Msg::System(_) => {}
        }
    }
    out
}

fn tool_result_snippet(result: &Value) -> String {
    let s = if let Some(stdout) = result.get("stdout").and_then(Value::as_str) {
        stdout.to_string()
    } else if let Some(exit_code) = result.get("exit_code") {
        exit_code.to_string()
    } else {
        result.to_string()
    };
    if s.len() > RESULT_TRUNCATE {
        let boundary = floor_char_boundary(s.as_str(), RESULT_TRUNCATE);
        format!("{}...", &s[..boundary])
    } else {
        s
    }
}

fn floor_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    let mut i = i;
    while !s.is_char_boundary(i) && i > 0 {
        i -= 1;
    }
    i
}

fn extract_json_from_response(response: &str) -> &str {
    let response = response.trim();
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if end > start {
                return response[start..=end].trim();
            }
        }
    }
    // ```json ... ``` の場合は中身を探す
    if let Some(inner) = response.strip_prefix("```json").or_else(|| response.strip_prefix("```")) {
        if let Some(end) = inner.find("```") {
            return inner[..end].trim();
        }
        return inner.trim();
    }
    response
}

fn parse_extraction_json(s: &str) -> Option<Value> {
    serde_json::from_str(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::msg::Msg;

    #[test]
    fn test_msgs_to_session_summary_empty() {
        let out = msgs_to_session_summary(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn test_msgs_to_session_summary_user_assistant() {
        let msgs = vec![Msg::user("hi"), Msg::assistant("hello")];
        let out = msgs_to_session_summary(&msgs);
        assert!(out.contains("USER: hi"));
        assert!(out.contains("ASSISTANT: hello"));
    }

    #[test]
    fn test_extract_json_from_response_plain() {
        let s = r#"{"content":"x","category":"general"}"#;
        assert_eq!(extract_json_from_response(s), s);
    }

    #[test]
    fn test_extract_json_from_response_with_markdown() {
        let s = "Some text\n{\"content\":\"y\"}\nmore";
        let j = extract_json_from_response(s);
        assert_eq!(j, "{\"content\":\"y\"}");
    }
}
