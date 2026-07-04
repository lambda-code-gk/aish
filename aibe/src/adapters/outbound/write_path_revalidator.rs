//! [`WritePathRevalidator`] の filesystem 実装。

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::adapters::outbound::tools::WritePathPolicy;
use crate::ports::outbound::{
    FileWriteConfig, ToolExecutionContext, WritePathRevalidateError, WritePathRevalidator,
};

/// `FileWriteConfig` 由来の write path 再検証。
#[derive(Debug, Clone)]
pub struct ConfigWritePathRevalidator {
    policy: WritePathPolicy,
}

impl ConfigWritePathRevalidator {
    pub fn from_config(config: &FileWriteConfig) -> Self {
        Self {
            policy: WritePathPolicy::from_config(config),
        }
    }
}

#[async_trait]
impl WritePathRevalidator for ConfigWritePathRevalidator {
    async fn revalidate_write_path(
        &self,
        ctx: &ToolExecutionContext,
        path: &Path,
        expect_absent: bool,
    ) -> Result<PathBuf, WritePathRevalidateError> {
        self.policy
            .revalidate_write_path(ctx, path, expect_absent)
            .await
            .map_err(|_| WritePathRevalidateError)
    }
}
