//! dry run 時の出力用ペイロード（usecase が返し、CLI が表示する）

use common::msg::Msg;

/// dry run で LLM を呼ばずに返す情報（プロファイル・モデル・システムプロンプト・メッセージ列など）
#[derive(Debug, Clone)]
pub struct DryRunInfo {
    pub profile_name: String,
    pub model_name: String,
    pub system_instruction: Option<String>,
    pub mode_name: Option<String>,
    /// leakscan が有効で manifest/reviewed 履歴を使っている場合 true
    pub leakscan_enabled: bool,
    pub tool_allowlist: Option<Vec<String>>,
    pub tools_enabled: Vec<String>,
    pub messages: Vec<Msg>,
}
