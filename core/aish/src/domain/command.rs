//! aish コマンドの enum（Command Pattern）
//!
//! 引数解析の結果を enum に落とし、match でディスパッチする。

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

    /// 実装済み: セッションのクリア
    Clear,

    /// コンソールログのロールアウト（SIGUSR1 相当）
    Rollout,

    /// コンソールログの記録を停止（rollout 後に mute）
    Mute,

    /// コンソールログの記録を再開
    Unmute,

    /// セッション再開（resume [<id>]）
    Resume { id: Option<String> },

    /// セッション一覧
    Sessions,

    /// システムプロンプト一覧（sysq list）
    SysqList,
    /// システムプロンプトを有効化（sysq enable id [id...]）
    SysqEnable { ids: Vec<String> },
    /// システムプロンプトを無効化（sysq disable id [id...]）
    SysqDisable { ids: Vec<String> },

    /// メモリ一覧（memory list）
    MemoryList,
    /// メモリ取得（memory get id [id...]）
    MemoryGet { ids: Vec<String> },
    /// メモリ削除（memory remove id [id...]）
    MemoryRemove { ids: Vec<String> },

    /// 未知のコマンド（エラー用）
    Unknown(String),
}

impl Command {
    /// コマンド名と引数から Command に解析する（sysq / resume は args を使用）
    pub fn parse_with_args(name: &str, args: &[String]) -> Self {
        if name == "resume" {
            let id = args.first().cloned();
            return Command::Resume { id };
        }
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
        if name == "memory" {
            match args.first().map(|s| s.as_str()) {
                Some("list") => return Command::MemoryList,
                Some("get") => return Command::MemoryGet { ids: args[1..].to_vec() },
                Some("remove") => return Command::MemoryRemove { ids: args[1..].to_vec() },
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("memory {}", sub).trim_end().to_string());
                }
            }
        }
        Self::parse(name)
    }

    /// 文字列を Command に解析する（サブコマンドなし）
    pub fn parse(s: &str) -> Self {
        match s {
            "truncate_console_log" => Command::TruncateConsoleLog,
            "clear" => Command::Clear,
            "rollout" => Command::Rollout,
            "mute" => Command::Mute,
            "unmute" => Command::Unmute,
            "resume" => Command::Resume { id: None },
            "sessions" => Command::Sessions,
            _ => Command::Unknown(s.to_string()),
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
    }

    #[test]
    fn test_parse_rollout() {
        let cmd = Command::parse("rollout");
        assert_eq!(cmd, Command::Rollout);
    }

    #[test]
    fn test_parse_mute() {
        let cmd = Command::parse("mute");
        assert_eq!(cmd, Command::Mute);
    }

    #[test]
    fn test_parse_unmute() {
        let cmd = Command::parse("unmute");
        assert_eq!(cmd, Command::Unmute);
    }

    #[test]
    fn test_parse_resume() {
        let cmd = Command::parse("resume");
        assert_eq!(cmd, Command::Resume { id: None });
    }

    #[test]
    fn test_parse_sessions() {
        let cmd = Command::parse("sessions");
        assert_eq!(cmd, Command::Sessions);
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

    #[test]
    fn test_parse_with_args_memory_list() {
        let cmd = Command::parse_with_args("memory", &["list".to_string()]);
        assert_eq!(cmd, Command::MemoryList);
    }

    #[test]
    fn test_parse_with_args_memory_get() {
        let cmd = Command::parse_with_args("memory", &["get".to_string(), "id1".to_string(), "id2".to_string()]);
        assert!(matches!(&cmd, Command::MemoryGet { ids } if ids == &["id1".to_string(), "id2".to_string()]));
    }

    #[test]
    fn test_parse_with_args_memory_remove() {
        let cmd = Command::parse_with_args("memory", &["remove".to_string(), "abc".to_string()]);
        assert!(matches!(&cmd, Command::MemoryRemove { ids } if ids == &["abc".to_string()]));
    }
}
