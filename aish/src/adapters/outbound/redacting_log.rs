//! handoff token 等の追加 secret を除去してから永続化する SessionLog ラッパ。

use aish_replay::{sanitize_log_text_with_secrets, LogEvent};

use crate::ports::outbound::{LogError, SessionLog};

pub struct RedactingSessionLog<L> {
    inner: L,
    secrets: Vec<String>,
}

impl<L> RedactingSessionLog<L> {
    pub fn new(inner: L, secrets: Vec<String>) -> Self {
        Self { inner, secrets }
    }

    fn redact_text(&self, input: &str) -> String {
        let refs: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
        sanitize_log_text_with_secrets(input, &refs)
    }

    fn redact_event(&self, event: &LogEvent) -> LogEvent {
        match event {
            LogEvent::CommandStart {
                command,
                args,
                command_index,
                started_at,
                kind,
            } => LogEvent::CommandStart {
                command: self.redact_text(command),
                args: args.iter().map(|arg| self.redact_text(arg)).collect(),
                command_index: *command_index,
                started_at: started_at.clone(),
                kind: *kind,
            },
            LogEvent::Stdout {
                data,
                command_index,
            } => LogEvent::Stdout {
                data: self.redact_text(data),
                command_index: *command_index,
            },
            LogEvent::Stderr {
                data,
                command_index,
            } => LogEvent::Stderr {
                data: self.redact_text(data),
                command_index: *command_index,
            },
            LogEvent::CommandEnd {
                command_index,
                exit_code,
                finished_at,
            } => LogEvent::CommandEnd {
                command_index: *command_index,
                exit_code: *exit_code,
                finished_at: finished_at.clone(),
            },
            LogEvent::Exit { code } => LogEvent::Exit { code: *code },
        }
    }
}

impl<L: SessionLog> SessionLog for RedactingSessionLog<L> {
    fn append(&mut self, event: &LogEvent) -> Result<(), LogError> {
        let redacted = self.redact_event(event);
        self.inner.append(&redacted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::JsonlFileLog;

    #[test]
    fn redacts_handoff_token_from_stdout_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.jsonl");
        let inner = JsonlFileLog::new(path.clone());
        let mut log = RedactingSessionLog::new(inner, vec!["super-secret-token".into()]);
        log.append(&LogEvent::Stdout {
            data: "token=super-secret-token\n".into(),
            command_index: Some(1),
        })
        .unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(!content.contains("super-secret-token"));
        assert!(content.contains("[REDACTED]"));
    }
}
