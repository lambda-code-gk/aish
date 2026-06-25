//! ツール実行時のリクエスト単位コンテキスト。
//!
//! **カレントディレクトリ方針**（`docs/architecture.md`）:
//! 相対パス・`.` 付き設定ルートの解決は [`ToolExecutionContext::base_dir`] を使う。
//! aibe プロセスの [`std::env::current_dir`] を直接参照してはいけない。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::Capability;
use crate::domain::ClientCwd;
use crate::ports::outbound::{
    CapabilityDenied, CapabilityPolicy, ClientToolGate, ShellExecApprovalGate,
};
use aibe_protocol::ClientProvidedToolSpec;

/// 1 回の `agent_turn` に紐づく実行コンテキスト。
#[derive(Clone)]
pub struct ToolExecutionContext {
    /// クライアント（例: `ai ask`）のカレントディレクトリ（絶対パス）。
    client_cwd: ClientCwd,
    turn_id: String,
    approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
    client_tool_gate: Option<Arc<dyn ClientToolGate>>,
    capability_policy: Option<Arc<dyn CapabilityPolicy>>,
    client_tools: Vec<ClientProvidedToolSpec>,
}

impl std::fmt::Debug for ToolExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolExecutionContext")
            .field("client_cwd", &self.client_cwd)
            .field("turn_id", &self.turn_id)
            .field("approval_gate", &self.approval_gate.is_some())
            .field("client_tool_gate", &self.client_tool_gate.is_some())
            .field(
                "capability_policy",
                &self.capability_policy.as_ref().map(|p| p.profile_name()),
            )
            .field("client_tools", &self.client_tools.len())
            .finish()
    }
}

impl PartialEq for ToolExecutionContext {
    fn eq(&self, other: &Self) -> bool {
        self.client_cwd == other.client_cwd
            && self.turn_id == other.turn_id
            && self.approval_gate.is_some() == other.approval_gate.is_some()
            && self.client_tool_gate.is_some() == other.client_tool_gate.is_some()
            && self.capability_policy.as_ref().map(|p| p.profile_name())
                == other.capability_policy.as_ref().map(|p| p.profile_name())
            && self.client_tools == other.client_tools
    }
}

impl Eq for ToolExecutionContext {}

impl ToolExecutionContext {
    pub fn new(client_cwd: ClientCwd) -> Self {
        Self {
            client_cwd,
            turn_id: String::new(),
            approval_gate: None,
            client_tool_gate: None,
            capability_policy: None,
            client_tools: Vec::new(),
        }
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = turn_id.into();
        self
    }

    pub fn with_approval_gate(mut self, gate: Arc<dyn ShellExecApprovalGate>) -> Self {
        self.approval_gate = Some(gate);
        self
    }

    pub fn with_client_tool_gate(mut self, gate: Arc<dyn ClientToolGate>) -> Self {
        self.client_tool_gate = Some(gate);
        self
    }

    pub fn with_capability_policy(mut self, policy: Arc<dyn CapabilityPolicy>) -> Self {
        self.capability_policy = Some(policy);
        self
    }

    pub fn with_client_tools(mut self, tools: Vec<ClientProvidedToolSpec>) -> Self {
        self.client_tools = tools;
        self
    }

    pub fn capability_policy(&self) -> Option<&Arc<dyn CapabilityPolicy>> {
        self.capability_policy.as_ref()
    }

    /// capability check（policy 未設定時は fail-closed）。
    pub fn require_capability(&self, capability: Capability) -> Result<(), CapabilityDenied> {
        match self.capability_policy.as_ref() {
            Some(policy) => policy.require(capability),
            None => Err(CapabilityDenied {
                capability,
                profile: "missing_policy".into(),
            }),
        }
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn approval_gate(&self) -> Option<&Arc<dyn ShellExecApprovalGate>> {
        self.approval_gate.as_ref()
    }

    pub fn client_tool_gate(&self) -> Option<&Arc<dyn ClientToolGate>> {
        self.client_tool_gate.as_ref()
    }

    pub fn client_tool_spec(&self, name: &str) -> Option<&ClientProvidedToolSpec> {
        self.client_tools.iter().find(|spec| spec.name == name)
    }

    /// 相対パス解決・`allowed_roots` の `.` 展開の基準ディレクトリ。
    pub fn base_dir(&self) -> &Path {
        self.client_cwd.as_path()
    }

    /// 相対パスを `base_dir` 基準で絶対パスにする。既に絶対ならそのまま。
    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir().join(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_at(path: &str) -> ToolExecutionContext {
        ToolExecutionContext::new(ClientCwd::new(PathBuf::from(path)).expect("absolute cwd"))
    }

    #[test]
    fn resolve_path_relative_uses_base_dir() {
        let ctx = ctx_at("/tmp/client");
        assert_eq!(
            ctx.resolve_path(Path::new("a/b.txt")),
            PathBuf::from("/tmp/client/a/b.txt")
        );
    }

    #[test]
    fn resolve_path_absolute_unchanged() {
        let ctx = ctx_at("/tmp/client");
        assert_eq!(
            ctx.resolve_path(Path::new("/etc/hosts")),
            PathBuf::from("/etc/hosts")
        );
    }
}
