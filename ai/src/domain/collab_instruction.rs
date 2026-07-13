use super::ExecutionMode;

pub const COLLABORATIVE_INSTRUCTION: &str = "Collaborative Mode is active. Delegate explicit human work with the human_task tool. Use shell_exec only when AISH itself should run an allowed command; do not confuse the two. The human_task objective must be concrete. Omit optional fields or use empty lists when their content is unknown. A done status and command evidence mean only that control returned, not that the task was automatically verified; re-observe the environment when verification is needed. Do not start human tasks in parallel or nest them.";

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
