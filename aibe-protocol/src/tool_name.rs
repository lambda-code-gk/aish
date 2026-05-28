//! 組み込みツール名（wire 契約）。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

pub const SHELL_EXEC: &str = "shell_exec";
pub const READ_FILE: &str = "read_file";
pub const LIST_DIR: &str = "list_dir";
pub const GREP: &str = "grep";
pub const GIT_DIFF: &str = "git_diff";
pub const GIT_STATUS: &str = "git_status";

pub const KNOWN_TOOLS: &[&str] = &[SHELL_EXEC, READ_FILE, LIST_DIR, GREP, GIT_DIFF, GIT_STATUS];

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

    pub fn list_dir() -> Self {
        Self(LIST_DIR.to_string())
    }

    pub fn grep() -> Self {
        Self(GREP.to_string())
    }

    pub fn git_diff() -> Self {
        Self(GIT_DIFF.to_string())
    }

    pub fn git_status() -> Self {
        Self(GIT_STATUS.to_string())
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

impl From<ToolName> for String {
    fn from(name: ToolName) -> Self {
        name.0
    }
}

impl Serialize for ToolName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ToolName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

pub fn is_known_tool(name: &str) -> bool {
    KNOWN_TOOLS.contains(&name)
}

/// wire 上の `Vec<String>` を検証済み `Vec<ToolName>` に変換する。
pub fn parse_tool_names(names: Vec<String>) -> Result<Vec<ToolName>, UnknownToolError> {
    names.iter().map(|s| s.parse()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_accepts_known_tools() {
        assert_eq!("read_file".parse(), Ok(ToolName::read_file()));
        assert_eq!("shell_exec".parse(), Ok(ToolName::shell_exec()));
        assert_eq!("list_dir".parse(), Ok(ToolName::list_dir()));
        assert_eq!("grep".parse(), Ok(ToolName::grep()));
        assert_eq!("git_diff".parse(), Ok(ToolName::git_diff()));
        assert_eq!("git_status".parse(), Ok(ToolName::git_status()));
    }

    #[test]
    fn from_str_rejects_unknown() {
        assert_eq!(
            "bogus".parse::<ToolName>(),
            Err(UnknownToolError("bogus".into()))
        );
    }

    #[test]
    fn serde_roundtrip() {
        let name = ToolName::read_file();
        let json = serde_json::to_string(&name).expect("serialize");
        assert_eq!(json, r#""read_file""#);
        let back: ToolName = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, name);
    }

    #[test]
    fn parse_tool_names_rejects_unknown() {
        let err = parse_tool_names(vec!["read_file".into(), "bogus".into()]).unwrap_err();
        assert_eq!(err, UnknownToolError("bogus".into()));
    }
}
