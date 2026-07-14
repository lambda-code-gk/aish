use super::ExecutionMode;

pub const COLLABORATIVE_INSTRUCTION: &str = "Collaborative Mode is active. Delegate explicit human work with the human_task tool. Use shell_exec only when AISH itself should run an allowed command; do not confuse the two. The human_task objective must be concrete. Omit optional fields or use empty lists when their content is unknown. A human_task result with status \"done\" and verified=false means ONLY that control returned from the Human Shell. It does NOT mean the objective succeeded, that software was installed, that files changed as requested, or that completion criteria were met. Never claim the work is complete, successful, finished, or installed until you independently re-observe the environment (for example with shell_exec or read-only tools) and confirm the required state yourself. If you cannot verify, say that control returned from the human task but completion has not been confirmed. Do not start human tasks in parallel or nest them.";

pub fn append_collaborative_instruction(
    existing: Option<String>,
    mode: ExecutionMode,
) -> Option<String> {
    if !mode.is_collaborative() {
        return existing;
    }
    Some(match existing {
        Some(value) if !value.trim().is_empty() => {
            format!("{value}\n\n{COLLABORATIVE_INSTRUCTION}")
        }
        _ => COLLABORATIVE_INSTRUCTION.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collaborative_instruction_forbids_unverified_completion_claims() {
        let text = COLLABORATIVE_INSTRUCTION.to_ascii_lowercase();
        assert!(text.contains("verified=false"));
        assert!(text.contains("control returned"));
        assert!(text.contains("completion has not been confirmed"));
        assert!(!text.contains("automatically verified"));
    }
}
