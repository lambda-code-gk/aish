//! システムプロンプト（sysq）list / enable / disable のユースケース

use common::error::Error;
use std::sync::Arc;

use crate::ports::outbound::{SysqListEntry, SysqRepository};

/// sysq コマンドのユースケース（list / enable / disable）
pub struct SysqUseCase {
    repo: Arc<dyn SysqRepository>,
}

impl SysqUseCase {
    pub fn new(repo: Arc<dyn SysqRepository>) -> Self {
        Self { repo }
    }

    /// 一覧を返す（表示は呼び出し側の責務）
    pub fn list(&self) -> Result<Vec<SysqListEntry>, Error> {
        self.repo.list_entries()
    }

    /// 指定IDを有効化する
    pub fn enable(&self, ids: &[String]) -> Result<(), Error> {
        self.repo.enable(ids)
    }

    /// 指定IDを無効化する
    pub fn disable(&self, ids: &[String]) -> Result<(), Error> {
        self.repo.disable(ids)
    }
}
