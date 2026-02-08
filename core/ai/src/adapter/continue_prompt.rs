//! エージェントループ上限到達時の「続けますか？」CLI 実装
//!
//! usecase は ContinueAfterLimitPrompt trait 経由でのみ利用する。

use crate::ports::outbound::ContinueAfterLimitPrompt;
use common::error::Error;
use std::io::{self, BufRead, Write};

/// 非対話用: 常に続行しない（false）を返す（CI 等でプロンプトを出さない）
pub struct NoContinuePrompt;

impl NoContinuePrompt {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoContinuePrompt {
    fn default() -> Self {
        Self::new()
    }
}

impl ContinueAfterLimitPrompt for NoContinuePrompt {
    fn ask_continue(&self) -> Result<bool, Error> {
        Ok(false)
    }
}

/// CLI で「続けますか？」を標準入出力で問い合わせる実装
pub struct CliContinuePrompt;

impl CliContinuePrompt {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CliContinuePrompt {
    fn default() -> Self {
        Self::new()
    }
}

impl ContinueAfterLimitPrompt for CliContinuePrompt {
    fn ask_continue(&self) -> Result<bool, Error> {
        eprint!("Agent loop reached the turn limit. Continue? [y/N]: ");
        let _ = io::stderr().flush();

        let stdin = io::stdin();
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| Error::io_msg(e.to_string()))?;

        let input = line.trim().to_lowercase();
        Ok(input == "y" || input == "yes")
    }
}
