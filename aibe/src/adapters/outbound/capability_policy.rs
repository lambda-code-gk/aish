//! built-in capability profile 実装。

use std::collections::HashSet;
use std::sync::Arc;

use crate::domain::Capability;
use crate::ports::outbound::CapabilityPolicy;

/// 固定 capability 集合を持つ policy。
#[derive(Debug, Clone)]
pub struct StaticCapabilityPolicy {
    profile: String,
    allowed: HashSet<Capability>,
}

impl StaticCapabilityPolicy {
    pub fn new(profile: impl Into<String>, allowed: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            profile: profile.into(),
            allowed: allowed.into_iter().collect(),
        }
    }

    /// 現行 CLI 互換: 全 capability を許可。
    pub fn local_full() -> Arc<dyn CapabilityPolicy> {
        Arc::new(Self::new(
            "local_full",
            [
                Capability::MemoryRead,
                Capability::MemoryWrite,
                Capability::MemoryArchive,
                Capability::MemoryRecipeRun,
                Capability::MemorySubscribe,
                Capability::AgentAsk,
                Capability::ShellPropose,
                Capability::ShellExecute,
                Capability::FileWrite,
            ],
        ))
    }

    /// memory write/archive を拒否するテスト fixture。
    pub fn memory_read_only() -> Arc<dyn CapabilityPolicy> {
        Arc::new(Self::new(
            "memory_read_only",
            [
                Capability::MemoryRead,
                Capability::MemoryRecipeRun,
                Capability::MemorySubscribe,
                Capability::AgentAsk,
                Capability::ShellPropose,
                Capability::ShellExecute,
            ],
        ))
    }

    /// shell execute を拒否するテスト fixture。
    pub fn memory_only() -> Arc<dyn CapabilityPolicy> {
        Arc::new(Self::new(
            "memory_only",
            [
                Capability::MemoryRead,
                Capability::MemoryWrite,
                Capability::MemoryArchive,
                Capability::MemoryRecipeRun,
                Capability::MemorySubscribe,
                Capability::AgentAsk,
                Capability::ShellPropose,
            ],
        ))
    }
}

impl CapabilityPolicy for StaticCapabilityPolicy {
    fn profile_name(&self) -> &str {
        &self.profile
    }

    fn allows(&self, capability: Capability) -> bool {
        self.allowed.contains(&capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_full_allows_all_capabilities() {
        let policy = StaticCapabilityPolicy::local_full();
        for cap in [
            Capability::MemoryRead,
            Capability::MemoryWrite,
            Capability::MemoryArchive,
            Capability::MemoryRecipeRun,
            Capability::MemorySubscribe,
            Capability::AgentAsk,
            Capability::ShellPropose,
            Capability::ShellExecute,
            Capability::FileWrite,
        ] {
            assert!(policy.allows(cap), "local_full should allow {cap:?}");
        }
    }

    #[test]
    fn memory_read_only_denies_write_and_archive() {
        let policy = StaticCapabilityPolicy::memory_read_only();
        assert!(policy.allows(Capability::MemoryRead));
        assert!(!policy.allows(Capability::MemoryWrite));
        assert!(!policy.allows(Capability::MemoryArchive));
        assert!(policy.allows(Capability::MemoryRecipeRun));
    }

    #[test]
    fn memory_only_denies_shell_execute() {
        let policy = StaticCapabilityPolicy::memory_only();
        assert!(policy.allows(Capability::ShellPropose));
        assert!(!policy.allows(Capability::ShellExecute));
        assert!(policy.allows(Capability::MemoryWrite));
    }
}
