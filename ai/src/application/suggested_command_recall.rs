//! turn 終了時の suggested command recall フロー。

use aibe_protocol::ClientResponse;

use crate::domain::{
    extract_shell_candidates_from_content, OutputFormat, SuggestedCommandCache,
    SuggestedCommandQueue,
};
use crate::ports::outbound::{SuggestedCommandRecallStore, SuggestedCommandRecallStoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecallGatingInput {
    pub config_enabled: bool,
    pub config_hint: bool,
    pub max_items: usize,
    pub quiet: bool,
    pub output_format: Option<OutputFormat>,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecallGating {
    pub enabled: bool,
    pub show_hint: bool,
    pub max_items: usize,
}

#[derive(Debug, Clone)]
pub struct RecallTurnContext {
    pub gating: RecallGating,
    pub ai_session_id: String,
    pub conversation_id: Option<String>,
    pub turn_id: String,
    pub captured_at: String,
    pub shell: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallPersistOutcome {
    pub saved_count: usize,
    pub hint: Option<String>,
}

pub fn resolve_recall_gating(input: RecallGatingInput) -> RecallGating {
    if input.output_format.is_some() {
        return RecallGating {
            enabled: false,
            show_hint: false,
            max_items: input.max_items,
        };
    }
    if !(input.stdin_tty && input.stdout_tty && input.stderr_tty) {
        return RecallGating {
            enabled: false,
            show_hint: false,
            max_items: input.max_items,
        };
    }
    if !input.config_enabled {
        return RecallGating {
            enabled: false,
            show_hint: false,
            max_items: input.max_items,
        };
    }
    RecallGating {
        enabled: true,
        show_hint: input.config_hint && !input.quiet,
        max_items: input.max_items,
    }
}

pub fn assistant_content_from_response(response: &ClientResponse) -> Option<&str> {
    match response {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => Some(assistant_message.content.as_str()),
        _ => None,
    }
}

pub fn persist_suggested_commands<S: SuggestedCommandRecallStore>(
    store: &S,
    ctx: &RecallTurnContext,
    assistant_content: &str,
) -> Result<RecallPersistOutcome, SuggestedCommandRecallStoreError> {
    if !ctx.gating.enabled {
        return Ok(RecallPersistOutcome {
            saved_count: 0,
            hint: None,
        });
    }
    let candidates = extract_shell_candidates_from_content(assistant_content, ctx.gating.max_items);
    if candidates.is_empty() {
        return Ok(RecallPersistOutcome {
            saved_count: 0,
            hint: None,
        });
    }
    let mut cache = store.load()?.unwrap_or_else(|| {
        SuggestedCommandCache::new(
            ctx.ai_session_id.clone(),
            ctx.shell.clone(),
            ctx.captured_at.clone(),
        )
    });
    cache.conversation_id = ctx.conversation_id.clone();
    cache.updated_at = ctx.captured_at.clone();
    cache.shell = ctx.shell.clone();
    cache.append_queue(SuggestedCommandQueue {
        turn_id: ctx.turn_id.clone(),
        captured_at: ctx.captured_at.clone(),
        candidates: candidates.clone(),
    });
    store.save(&cache)?;
    let hint = if ctx.gating.show_hint {
        Some(format!(
            "ai: {} suggested command{} ready. Alt+. / Alt+, cycle proposals.",
            candidates.len(),
            if candidates.len() == 1 { "" } else { "s" }
        ))
    } else {
        None
    };
    Ok(RecallPersistOutcome {
        saved_count: candidates.len(),
        hint,
    })
}

pub fn recall_next_command<S: SuggestedCommandRecallStore>(
    store: &S,
) -> Result<Option<String>, SuggestedCommandRecallStoreError> {
    let Some(mut cache) = store.load()? else {
        return Ok(None);
    };
    let next = cache.next_candidate();
    if next.is_some() {
        store.save(&cache)?;
    }
    Ok(next)
}

pub fn recall_prev_command<S: SuggestedCommandRecallStore>(
    store: &S,
) -> Result<Option<String>, SuggestedCommandRecallStoreError> {
    let Some(mut cache) = store.load()? else {
        return Ok(None);
    };
    let prev = cache.prev_candidate();
    if prev.is_some() {
        store.save(&cache)?;
    }
    Ok(prev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::OutputFormat;

    #[test]
    fn quiet_mode_suppresses_hint_without_disabling_recall_cache() {
        let gating = resolve_recall_gating(RecallGatingInput {
            config_enabled: true,
            config_hint: true,
            max_items: 8,
            quiet: true,
            output_format: None,
            stdin_tty: true,
            stdout_tty: true,
            stderr_tty: true,
        });
        assert!(gating.enabled);
        assert!(!gating.show_hint);
    }

    #[test]
    fn structured_output_disables_suggested_command_recall() {
        let gating = resolve_recall_gating(RecallGatingInput {
            config_enabled: true,
            config_hint: true,
            max_items: 8,
            quiet: false,
            output_format: Some(OutputFormat::Json),
            stdin_tty: true,
            stdout_tty: true,
            stderr_tty: true,
        });
        assert!(!gating.enabled);
        assert!(!gating.show_hint);
    }

    #[test]
    fn non_tty_disables_suggested_command_recall() {
        let gating = resolve_recall_gating(RecallGatingInput {
            config_enabled: true,
            config_hint: true,
            max_items: 8,
            quiet: false,
            output_format: None,
            stdin_tty: false,
            stdout_tty: true,
            stderr_tty: true,
        });
        assert!(!gating.enabled);
    }
}
