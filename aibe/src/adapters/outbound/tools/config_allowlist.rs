//! 設定ベースの `shell_exec` allowlist。

use std::path::Path;

use crate::ports::outbound::{CommandPolicy, ShellExecApprovalMode, ShellExecConfig};

pub struct ConfigAllowlistPolicy {
    config: ShellExecConfig,
}

impl ConfigAllowlistPolicy {
    pub fn new(config: ShellExecConfig) -> Self {
        Self { config }
    }

    fn normalize_command(command: &str) -> String {
        let mut s = command.trim().to_string();
        if s.starts_with("./") {
            s = s[2..].to_string();
        }
        s
    }

    fn basename(command: &str) -> String {
        Path::new(command)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(command)
            .to_string()
    }
}

impl CommandPolicy for ConfigAllowlistPolicy {
    fn shell_exec_enabled(&self) -> bool {
        self.config.enabled
    }

    fn is_command_allowed(&self, command: &str) -> bool {
        if !self.config.enabled || self.config.allowed_commands.is_empty() {
            return false;
        }
        let normalized = Self::normalize_command(command);
        let base = Self::basename(&normalized);
        for entry in &self.config.allowed_commands {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if entry.contains('/') {
                if normalized == entry {
                    return true;
                }
            } else if base == entry {
                return true;
            }
        }
        false
    }

    fn shell_exec_approval_mode(&self) -> ShellExecApprovalMode {
        self.config.approval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(allowed: &[&str]) -> ConfigAllowlistPolicy {
        ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: allowed.iter().map(|s| (*s).to_string()).collect(),
            approval: ShellExecApprovalMode::Always,
        })
    }

    #[test]
    fn basename_match() {
        let p = policy(&["git", "ls"]);
        assert!(p.is_command_allowed("git"));
        assert!(p.is_command_allowed("./git"));
        assert!(!p.is_command_allowed("curl"));
    }

    #[test]
    fn absolute_match() {
        let p = policy(&["/usr/bin/rg"]);
        assert!(p.is_command_allowed("/usr/bin/rg"));
        assert!(!p.is_command_allowed("rg"));
    }

    #[test]
    fn empty_allowlist_denies() {
        let p = ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec![],
            approval: ShellExecApprovalMode::Always,
        });
        assert!(!p.is_command_allowed("ls"));
    }
}
