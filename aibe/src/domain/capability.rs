//! クライアント capability（memory / shell / agent 権限の分離）。

use aibe_protocol::MemoryOperationDto;

/// 将来の multi-client 向け capability。v1 は local runtime の boundary check に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    MemoryRead,
    MemoryWrite,
    MemoryArchive,
    MemoryRecipeRun,
    MemorySubscribe,
    AgentAsk,
    ShellPropose,
    ShellExecute,
    FileWrite,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MemoryRead => "memory:read",
            Self::MemoryWrite => "memory:write",
            Self::MemoryArchive => "memory:archive",
            Self::MemoryRecipeRun => "memory:recipe_run",
            Self::MemorySubscribe => "memory:subscribe",
            Self::AgentAsk => "agent:ask",
            Self::ShellPropose => "shell:propose",
            Self::ShellExecute => "shell:execute",
            Self::FileWrite => "file:write",
        }
    }

    /// wire 文字列から capability を解釈する。
    pub fn parse_wire(s: &str) -> Option<Self> {
        match s {
            "memory:read" => Some(Self::MemoryRead),
            "memory:write" => Some(Self::MemoryWrite),
            "memory:archive" => Some(Self::MemoryArchive),
            "memory:recipe_run" => Some(Self::MemoryRecipeRun),
            "memory:subscribe" => Some(Self::MemorySubscribe),
            "agent:ask" => Some(Self::AgentAsk),
            "shell:propose" => Some(Self::ShellPropose),
            "shell:execute" => Some(Self::ShellExecute),
            "file:write" => Some(Self::FileWrite),
            _ => None,
        }
    }
}

/// `MemoryApply` の operation 種別に必要な capability。
pub fn required_capability_for_memory_operation(operation: &MemoryOperationDto) -> Capability {
    match operation {
        MemoryOperationDto::Add(_) => Capability::MemoryWrite,
        MemoryOperationDto::ClearKind(_) | MemoryOperationDto::Archive(_) => {
            Capability::MemoryArchive
        }
    }
}

/// recipe apply 時に proposals 全体で必要な capability 集合（重複なし）。
pub fn required_capabilities_for_memory_operations<'a>(
    operations: impl IntoIterator<Item = &'a MemoryOperationDto>,
) -> Vec<Capability> {
    let mut caps = Vec::new();
    for op in operations {
        let cap = required_capability_for_memory_operation(op);
        if !caps.contains(&cap) {
            caps.push(cap);
        }
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::{
        MemoryOperationAdd, MemoryOperationArchive, MemoryOperationClearKind, MemoryScopeDto,
    };

    #[test]
    fn add_requires_memory_write() {
        let op = MemoryOperationDto::Add(MemoryOperationAdd {
            kind: "goal".into(),
            scope: None,
            inject: None,
            status: None,
            text: "x".into(),
            make_active: None,
        });
        assert_eq!(
            required_capability_for_memory_operation(&op),
            Capability::MemoryWrite
        );
    }

    #[test]
    fn archive_requires_memory_archive() {
        let op = MemoryOperationDto::Archive(MemoryOperationArchive {
            id: "e1".into(),
            expected_version: None,
        });
        assert_eq!(
            required_capability_for_memory_operation(&op),
            Capability::MemoryArchive
        );
    }

    #[test]
    fn clear_kind_requires_memory_archive() {
        let op = MemoryOperationDto::ClearKind(MemoryOperationClearKind {
            kind: "goal".into(),
            scope: MemoryScopeDto::Project,
        });
        assert_eq!(
            required_capability_for_memory_operation(&op),
            Capability::MemoryArchive
        );
    }
}
