//! human shell 用 handoff token 検証（0055）。aish は ai 非依存で store を読む。

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct HandoffShellSession {
    generation: u32,
    token_hash: String,
}

pub fn hash_handoff_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn validate_shell_token(
    sessions: &[HandoffShellSession],
    token: &str,
    generation: u32,
) -> bool {
    let current_max = sessions
        .iter()
        .map(|session| session.generation)
        .max()
        .unwrap_or(0);
    if generation != current_max {
        return false;
    }
    sessions
        .iter()
        .rev()
        .find(|session| session.generation == current_max)
        .is_some_and(|session| hash_handoff_token(token) == session.token_hash)
}

pub(crate) fn load_shell_sessions(handoff_dir: &Path) -> anyhow::Result<Vec<HandoffShellSession>> {
    let path = handoff_dir.join("shell_sessions.jsonl");
    let raw = std::fs::read_to_string(path)?;
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line.trim())
                .map_err(|error| anyhow::anyhow!("invalid shell session record: {error}"))
        })
        .collect()
}

pub fn validate_handoff_shell_credentials(handoff_dir: &Path) -> anyhow::Result<()> {
    let token = std::env::var("AISH_HANDOFF_TOKEN")?;
    let generation = std::env::var("AISH_HANDOFF_CONTEXT_VERSION")?
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("invalid AISH_HANDOFF_CONTEXT_VERSION"))?;
    let sessions = load_shell_sessions(handoff_dir)?;
    if validate_shell_token(&sessions, &token, generation) {
        Ok(())
    } else {
        anyhow::bail!("handoff token does not match current shell session generation")
    }
}

pub fn process_is_alive(process_id: u32) -> bool {
    if process_id == 0 {
        return false;
    }
    if unsafe { libc::kill(process_id as i32, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn may_transfer_lease_to_human_shell(owner_client_id: &str, owner_process_id: u32) -> bool {
    owner_client_id.starts_with("ai-parent-") || !process_is_alive(owner_process_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_shell_token_accepts_matching_generation() {
        let token = "secret";
        let sessions = vec![HandoffShellSession {
            generation: 1,
            token_hash: hash_handoff_token(token),
        }];
        assert!(validate_shell_token(&sessions, token, 1));
        assert!(!validate_shell_token(&sessions, token, 2));
        assert!(!validate_shell_token(&sessions, "wrong", 1));
    }
}
