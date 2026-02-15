//! ツール実行の Ports & Adapters（trait で副作用隔離）
//!
//! ToolRegistry で name -> Box<dyn Tool> を解決し、ToolContext は session dir / fs / process / clock 等の port を束ねる。
//! Tool trait は Outbound ポートとして ports/outbound からも re-export される。

use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// LLM API に渡すツール定義（名前・説明・パラメータスキーマ）
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// ツール実行エラー（ドメイン層）
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// ツール実行コンテキスト（session dir / fs / process / clock 等の port を束ねる）
/// 最小限でよい。必要に応じて adapter を追加する。
#[derive(Clone)]
pub struct ToolContext {
    /// セッションディレクトリ（オプション）
    pub session_dir: Option<std::path::PathBuf>,
    /// 実行を許可するコマンドのルールリスト
    pub command_allow_rules: Vec<CommandAllowRule>,
    /// 危険操作を許可するフラグ（承認済みの場合に true）
    /// デフォルトは false。usecase が承認を得た後に with_allow_unsafe(true) で複製する。
    pub allow_unsafe: bool,
    /// メモリ用: プロジェクト固有の記憶ディレクトリ（.aish/memory）。None のときはグローバルのみ使用。
    pub memory_dir_project: Option<std::path::PathBuf>,
    /// メモリ用: グローバル記憶ディレクトリ（例: $AISH_HOME/memory）
    pub memory_dir_global: Option<std::path::PathBuf>,
}

/// コマンド実行許可ルール
#[derive(Debug, Clone)]
pub enum CommandAllowRule {
    /// 正規表現マッチ
    Regex(Regex),
    /// 前方一致（リテラル）
    Prefix(String),
    /// 否定マッチ（正規表現）
    NotRegex(Regex),
    /// 否定マッチ（前方一致）
    NotPrefix(String),
}

impl ToolContext {
    pub fn new(session_dir: Option<std::path::PathBuf>) -> Self {
        Self {
            session_dir,
            command_allow_rules: Vec::new(),
            allow_unsafe: false,
            memory_dir_project: None,
            memory_dir_global: None,
        }
    }

    pub fn with_command_allow_rules(mut self, rules: Vec<CommandAllowRule>) -> Self {
        self.command_allow_rules = rules;
        self
    }

    /// 危険操作を許可するコンテキストを返す（承認済みの場合に使用）
    pub fn with_allow_unsafe(mut self, allow: bool) -> Self {
        self.allow_unsafe = allow;
        self
    }

    /// メモリ用ディレクトリを設定（プロジェクト優先・グローバル）
    pub fn with_memory_dirs(
        mut self,
        project: Option<std::path::PathBuf>,
        global: Option<std::path::PathBuf>,
    ) -> Self {
        self.memory_dir_project = project;
        self.memory_dir_global = global;
        self
    }
}

/// ツールのトレイト（Outbound ポート）
pub trait Tool: Send + Sync {
    /// ツール名（API の name と一致させる）
    fn name(&self) -> &'static str;
    /// LLM 用の説明（空でも可）
    fn description(&self) -> &'static str {
        ""
    }
    /// パラメータの JSON Schema（None の場合は空オブジェクトを API に渡す）
    fn parameters_schema(&self) -> Option<Value> {
        None
    }
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

    /// 登録済みツールの定義一覧（LLM の make_request_payload に渡す用）
    pub fn list_definitions(&self) -> Vec<ToolDef> {
        self.tools
            .values()
            .map(|t| ToolDef {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t
                    .parameters_schema()
                    .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} })),
            })
            .collect()
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
    fn description(&self) -> &'static str {
        "Echo back the given input as JSON. Use for testing or passing through data."
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Message to echo back" },
                "input": { "type": "string", "description": "Input to echo (alias for message)" }
            }
        }))
    }
    fn call(&self, args: Value, _ctx: &ToolContext) -> Result<Value, ToolError> {
        let mut wrapped_args = args.clone();
        if let Some(obj) = wrapped_args.as_object_mut() {
            for (_, val) in obj.iter_mut() {
                if let Some(s) = val.as_str() {
                    *val = serde_json::json!(format!("[[[ {} ]]]", s));
                }
            }
        }
        Ok(serde_json::json!({ "output": wrapped_args }))
    }
}

/// 前方一致判定: ルール文字列の後ろにスペースを付けて判定する。
/// これにより `ls` は `ls` または `ls ` にマッチするが `lss` にはマッチしない。
fn prefix_matches(prefix: &str, command: &str) -> bool {
    command == prefix || command.starts_with(&format!("{} ", prefix))
}

/// コマンドが許可リストにマッチするか判定する（純粋関数）
///
/// denylist（NotRegex/NotPrefix）が先に評価され、一致すれば即座に false。
/// 次に allowlist（Regex/Prefix）のいずれかに一致すれば true。
/// どちらにも一致しなければ false。
///
/// Prefix/NotPrefix の前方一致は、ルール文字列の後ろにスペースを付けて判定する。
/// 例: ルール `ls` はコマンド行が `ls` または `ls ` で始まる場合にマッチ（`lss` はマッチしない）。
pub fn is_command_allowed(command: &str, rules: &[CommandAllowRule]) -> bool {
    // 否定ルールが一つでもマッチしたら即座に不許可
    let denied = rules.iter().any(|rule| match rule {
        CommandAllowRule::NotRegex(re) => re.is_match(command),
        CommandAllowRule::NotPrefix(prefix) => prefix_matches(prefix, command),
        _ => false,
    });
    if denied {
        return false;
    }

    // 肯定ルールのいずれかにマッチすれば許可
    rules.iter().any(|rule| match rule {
        CommandAllowRule::Regex(re) => re.is_match(command),
        CommandAllowRule::Prefix(prefix) => prefix_matches(prefix, command),
        _ => false,
    })
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
        // EchoTool wraps string values with [[[ and ]]]
        assert_eq!(r["output"]["msg"].as_str(), Some("[[[ hi ]]]"));
    }

    #[test]
    fn test_list_definitions() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(EchoTool::new()));
        reg.register(Arc::new(StubTool));
        let defs = reg.list_definitions();
        assert_eq!(defs.len(), 2);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"stub"));
        let echo_def = defs.iter().find(|d| d.name == "echo").unwrap();
        assert!(!echo_def.description.is_empty());
        assert!(echo_def.parameters.get("properties").is_some());
    }

    #[test]
    fn test_is_command_allowed() {
        let rules = vec![
            CommandAllowRule::Regex(Regex::new(r"^echo .*").unwrap()),
            CommandAllowRule::Prefix("ls".to_string()),
            CommandAllowRule::Prefix("sed".to_string()),
            CommandAllowRule::NotRegex(Regex::new(r"sed .*-i ").unwrap()),
        ];

        assert!(is_command_allowed("echo hello", &rules));
        assert!(is_command_allowed("ls", &rules));
        assert!(is_command_allowed("ls -la", &rules));
        assert!(is_command_allowed("sed 's/a/b/' file", &rules));
        assert!(!is_command_allowed("sed -i 's/a/b/' file", &rules));
        // sed --in-place は "sed .*-i " にマッチしないので許可される
        assert!(!is_command_allowed("rm -rf /", &rules));
        // 前方一致は「ルール+スペース」で判定するため、ls では lss はマッチしない
        assert!(!is_command_allowed("lss", &rules));
    }

    #[test]
    fn test_is_command_allowed_empty_rules() {
        let rules: Vec<CommandAllowRule> = vec![];
        // ルールがない場合は全て不許可
        assert!(!is_command_allowed("echo hello", &rules));
        assert!(!is_command_allowed("ls", &rules));
    }

    #[test]
    fn test_tool_context_clone() {
        let ctx = ToolContext::new(Some("/tmp".into()))
            .with_command_allow_rules(vec![CommandAllowRule::Prefix("ls".to_string())]);
        let ctx2 = ctx.clone().with_allow_unsafe(true);
        assert!(!ctx.allow_unsafe);
        assert!(ctx2.allow_unsafe);
    }
}
