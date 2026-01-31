//! ツール実行の Ports & Adapters（trait で副作用隔離）
//!
//! ToolRegistry で name -> Box<dyn Tool> を解決し、ToolContext は session dir / fs / process / clock 等の port を束ねる。

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// ツール実行エラー（ドメイン層）
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

/// ツール実行コンテキスト（session dir / fs / process / clock 等の port を束ねる）
/// 最小限でよい。必要に応じて adapter を追加する。
pub struct ToolContext {
    /// セッションディレクトリ（オプション）
    pub session_dir: Option<std::path::PathBuf>,
}

impl ToolContext {
    pub fn new(session_dir: Option<std::path::PathBuf>) -> Self {
        Self { session_dir }
    }
}

/// ツールのトレイト
pub trait Tool: Send + Sync {
    /// ツール名（API の name と一致させる）
    fn name(&self) -> &'static str;
    /// 引数とコンテキストで実行し、JSON 結果を返す
    fn call(&self, args: Value, ctx: &ToolContext) -> Result<Value, ToolError>;
}

/// ツール名で解決するレジストリ
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools
            .insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn call(
        &self,
        name: &str,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<Value, ToolError> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        tool.call(args, ctx)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// テスト・デモ用: 引数をそのまま返すツール（API 名 "echo"）
pub struct EchoTool;

impl EchoTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EchoTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        Ok(serde_json::json!({ "output": args }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubTool;

    impl Tool for StubTool {
        fn name(&self) -> &'static str {
            "stub"
        }
        fn call(&self, _args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
            Ok(serde_json::json!({"ok": true}))
        }
    }

    #[test]
    fn test_registry_register_get() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(StubTool));
        assert!(reg.get("stub").is_some());
        assert!(reg.get("unknown").is_none());
    }

    #[test]
    fn test_registry_call() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(StubTool));
        let ctx = ToolContext::new(None);
        let r = reg.call("stub", serde_json::json!({}), &ctx);
        assert!(r.is_ok());
        assert_eq!(r.unwrap(), serde_json::json!({"ok": true}));
    }

    #[test]
    fn test_registry_call_not_found() {
        let reg = ToolRegistry::new();
        let ctx = ToolContext::new(None);
        let r = reg.call("unknown", serde_json::json!({}), &ctx);
        assert!(matches!(r, Err(ToolError::NotFound(_))));
    }

    #[test]
    fn test_echo_tool() {
        let echo = EchoTool::new();
        assert_eq!(echo.name(), "echo");
        let ctx = ToolContext::new(None);
        let r = echo.call(serde_json::json!({"msg": "hi"}), &ctx).unwrap();
        assert_eq!(r["output"]["msg"].as_str(), Some("hi"));
    }
}
