//! aish コマンドの enum（Command Pattern）
//!
//! 引数解析の結果を enum に落とし、match でディスパッチする。
//! 未実装コマンドは各分岐として明示する。

/// aish のサブコマンド
///
/// コマンドなし = 対話シェル起動。それ以外は文字列から解析。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// ヘルプ表示
    Help,

    /// 対話シェルを起動（コマンド未指定時）
    Shell,

    /// 実装済み: コンソールバッファ・ログのロールオーバー
    TruncateConsoleLog,

    // --- 未実装（usage に記載されているもの）---
    /// 未実装: セッション再開
    Resume,
    /// 未実装: セッション一覧
    Sessions,
    /// 未実装: ロールアウト
    Rollout,
    /// 未実装: クリア
    Clear,
    /// 未実装: 一覧表示
    Ls,
    /// 未実装: 最後の part 削除
    RmLast,
    /// 未実装: メモリ操作
    Memory,
    /// 未実装: モデル一覧
    Models,

    /// システムプロンプト一覧（sysq list）
    SysqList,
    /// システムプロンプトを有効化（sysq enable id [id...]）
    SysqEnable { ids: Vec<String> },
    /// システムプロンプトを無効化（sysq disable id [id...]）
    SysqDisable { ids: Vec<String> },

    /// 未知のコマンド（エラー用）
    Unknown(String),
}

impl Command {
    /// コマンド名と引数から Command に解析する（sysq サブコマンドは args を使用）
    pub fn parse_with_args(name: &str, args: &[String]) -> Self {
        if name == "sysq" {
            match args.first().map(|s| s.as_str()) {
                Some("list") => return Command::SysqList,
                Some("enable") => return Command::SysqEnable { ids: args[1..].to_vec() },
                Some("disable") => return Command::SysqDisable { ids: args[1..].to_vec() },
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("sysq {}", sub).trim_end().to_string());
                }
            }
        }
        Self::parse(name)
    }

    /// 文字列を Command に解析する（サブコマンドなし）
    pub fn parse(s: &str) -> Self {
        match s {
            "truncate_console_log" => Command::TruncateConsoleLog,
            "resume" => Command::Resume,
            "sessions" => Command::Sessions,
            "rollout" => Command::Rollout,
            "clear" => Command::Clear,
            "ls" => Command::Ls,
            "rm_last" => Command::RmLast,
            "memory" => Command::Memory,
            "models" => Command::Models,
            _ => Command::Unknown(s.to_string()),
        }
    }

    /// 未実装かどうか
    #[allow(dead_code)] // 将来のディスパッチ簡略化で使用
    pub fn is_unimplemented(&self) -> bool {
        matches!(
            self,
            Command::Resume
                | Command::Sessions
                | Command::Rollout
                | Command::Ls
                | Command::RmLast
                | Command::Memory
                | Command::Models
        )
    }

    /// 実装済みかどうか
    #[allow(dead_code)] // 将来のディスパッチ簡略化で使用
    pub fn is_implemented(&self) -> bool {
        matches!(self, Command::TruncateConsoleLog | Command::Clear)
    }

    /// エラーメッセージ用の名前
    pub fn as_str(&self) -> &str {
        match self {
            Command::Help => "(help)",
            Command::Shell => "(shell)",
            Command::TruncateConsoleLog => "truncate_console_log",
            Command::Resume => "resume",
            Command::Sessions => "sessions",
            Command::Rollout => "rollout",
            Command::Clear => "clear",
            Command::Ls => "ls",
            Command::RmLast => "rm_last",
            Command::Memory => "memory",
            Command::Models => "models",
            Command::SysqList => "sysq list",
            Command::SysqEnable { .. } => "sysq enable",
            Command::SysqDisable { .. } => "sysq disable",
            Command::Unknown(s) => s.as_str(),
        }
    }

    /// sysq 系コマンドかどうか
    #[allow(dead_code)]
    pub fn is_sysq(&self) -> bool {
        matches!(self, Command::SysqList | Command::SysqEnable { .. } | Command::SysqDisable { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_truncate_console_log() {
        let cmd = Command::parse("truncate_console_log");
        assert_eq!(cmd, Command::TruncateConsoleLog);
        assert!(cmd.is_implemented());
    }

    #[test]
    fn test_parse_unimplemented_commands() {
        assert_eq!(Command::parse("resume"), Command::Resume);
        assert_eq!(Command::parse("sessions"), Command::Sessions);
        assert!(Command::parse("resume").is_unimplemented());
    }

    #[test]
    fn test_parse_unknown() {
        let cmd = Command::parse("unknown_cmd");
        assert!(matches!(cmd, Command::Unknown(s) if s == "unknown_cmd"));
    }

    #[test]
    fn test_parse_with_args_sysq_list() {
        let cmd = Command::parse_with_args("sysq", &["list".to_string()]);
        assert_eq!(cmd, Command::SysqList);
        assert!(cmd.is_sysq());
    }

    #[test]
    fn test_parse_with_args_sysq_enable() {
        let cmd = Command::parse_with_args("sysq", &["enable".to_string(), "dev/coding".to_string()]);
        assert!(matches!(&cmd, Command::SysqEnable { ids } if ids == &["dev/coding".to_string()]));
        assert!(cmd.is_sysq());
    }

    #[test]
    fn test_parse_with_args_sysq_disable() {
        let cmd = Command::parse_with_args("sysq", &["disable".to_string(), "a".to_string(), "b".to_string()]);
        assert!(matches!(&cmd, Command::SysqDisable { ids } if ids == &["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn test_parse_with_args_sysq_unknown_subcommand() {
        let cmd = Command::parse_with_args("sysq", &["invalid".to_string()]);
        assert!(matches!(cmd, Command::Unknown(_)));
    }
}
