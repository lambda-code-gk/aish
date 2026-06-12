//! memory space ID の生成・検証（wire 共有）。

/// `memory_space_id` が path component として安全か検証する。
pub fn is_valid_memory_space_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        // "." / ".." は親ディレクトリ参照になるため拒否（dot のみの ID を全て弾く）
        && !id.chars().all(|c| c == '.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// session 由来の暫定 fallback ID（非推奨）。
pub fn legacy_session_memory_space_id(session_id: &str) -> String {
    format!("legacy_session_{session_id}")
}

/// `project_key` から安定した project-backed `memory_space_id` を生成する。
pub fn project_memory_space_id(project_key: &str) -> String {
    format!("project_{:016x}", fnv1a64(project_key))
}

fn fnv1a64(s: &str) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_id_is_stable() {
        let a = project_memory_space_id("/tmp/proj");
        let b = project_memory_space_id("/tmp/proj");
        assert_eq!(a, b);
        assert!(is_valid_memory_space_id(&a));
    }

    #[test]
    fn legacy_session_id_is_valid() {
        let id = legacy_session_memory_space_id("sess_001");
        assert!(is_valid_memory_space_id(&id));
        assert!(id.starts_with("legacy_session_"));
    }

    #[test]
    fn rejects_invalid_ids() {
        assert!(!is_valid_memory_space_id(""));
        assert!(!is_valid_memory_space_id("../escape"));
        assert!(!is_valid_memory_space_id("has/slash"));
    }

    #[test]
    fn rejects_dot_only_ids() {
        assert!(!is_valid_memory_space_id("."));
        assert!(!is_valid_memory_space_id(".."));
        assert!(!is_valid_memory_space_id("..."));
        assert!(is_valid_memory_space_id("ctx.a"));
        assert!(is_valid_memory_space_id("v1.2"));
    }
}
