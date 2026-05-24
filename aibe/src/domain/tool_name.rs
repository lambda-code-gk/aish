//! 組み込みツール名（正本）。

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

pub const SHELL_EXEC: &str = "shell_exec";
pub const READ_FILE: &str = "read_file";

pub const KNOWN_TOOLS: &[&str] = &[SHELL_EXEC, READ_FILE];

#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown tool: {0}")]
pub struct UnknownToolError(pub String);

/// 組み込みツール名。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolName(String);

impl ToolName {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn read_file() -> Self {
        Self(READ_FILE.to_string())
    }

    pub fn shell_exec() -> Self {
        Self(SHELL_EXEC.to_string())
    }
}

impl FromStr for ToolName {
    type Err = UnknownToolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if KNOWN_TOOLS.contains(&s) {
            Ok(Self(s.to_string()))
        } else {
            Err(UnknownToolError(s.to_string()))
        }
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn is_known_tool(name: &str) -> bool {
    KNOWN_TOOLS.contains(&name)
}
