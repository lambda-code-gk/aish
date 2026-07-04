//! 承認後の write path 再検証 port。

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::ToolExecutionContext;

/// write path 再検証エラー（`stale_file` にマップ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WritePathRevalidateError;

#[async_trait]
pub trait WritePathRevalidator: Send + Sync {
    async fn revalidate_write_path(
        &self,
        ctx: &ToolExecutionContext,
        path: &Path,
        expect_absent: bool,
    ) -> Result<PathBuf, WritePathRevalidateError>;
}
