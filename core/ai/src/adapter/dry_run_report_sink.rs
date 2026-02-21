//! dry run 結果の出力 adapter（stdout に人間向けフォーマットで出力）

use crate::domain::DryRunInfo;
use crate::ports::outbound::DryRunReportSink;
use common::error::Error;

/// dry run の結果を stdout に出力する adapter
pub struct StdoutDryRunReportSink;

impl StdoutDryRunReportSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdoutDryRunReportSink {
    fn default() -> Self {
        Self::new()
    }
}

impl DryRunReportSink for StdoutDryRunReportSink {
    fn report(&self, info: &DryRunInfo) -> Result<(), Error> {
        println!("=== ai dry run ===");
        println!("profile: {}", info.profile_name);
        println!("model: {}", info.model_name);
        if let Some(ref s) = info.system_instruction {
            println!("system_instruction: |");
            for line in s.lines() {
                println!("  {}", line);
            }
        } else {
            println!("system_instruction: (none)");
        }
        match &info.mode_name {
            Some(m) => println!("mode: {}", m),
            None => println!("mode: (none)"),
        }
        match &info.tool_allowlist {
            Some(list) => println!("tool_allowlist: [{}]", list.join(", ")),
            None => println!("tool_allowlist: (all)"),
        }
        println!("tools_enabled: [{}]", info.tools_enabled.join(", "));
        println!("--- messages ({} total) ---", info.messages.len());
        for (i, m) in info.messages.iter().enumerate() {
            let (role, content) = match m {
                common::msg::Msg::System(s) => ("system", s.as_str()),
                common::msg::Msg::User(s) => ("user", s.as_str()),
                common::msg::Msg::Assistant(s) => ("assistant", s.as_str()),
                common::msg::Msg::ToolCall { call_id, name, args, .. } => {
                    let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
                    println!("  [{}] tool_call id={} name={} args={}", i, call_id, name, args_str);
                    continue;
                }
                common::msg::Msg::ToolResult { call_id, name, result } => {
                    let res_str = serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string());
                    println!("  [{}] tool_result id={} name={} result={}", i, call_id, name, res_str);
                    continue;
                }
            };
            println!("  [{}] {}:", i, role);
            for line in content.lines() {
                println!("    {}", line);
            }
        }
        println!("=== end dry run ===");
        Ok(())
    }
}
