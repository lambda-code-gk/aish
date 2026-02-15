//! メモリ list / get のユースケース

use common::error::Error;
use std::sync::Arc;

use crate::domain::{MemoryEntry, MemoryListEntry};
use crate::ports::outbound::MemoryRepository;

/// memory コマンドのユースケース（list / get）
pub struct MemoryUseCase {
    repo: Arc<dyn MemoryRepository>,
}

impl MemoryUseCase {
    pub fn new(repo: Arc<dyn MemoryRepository>) -> Self {
        Self { repo }
    }

    /// 一覧を返す（project + global をマージ）
    pub fn list(&self) -> Result<Vec<MemoryListEntry>, Error> {
        let (project_dir, global_dir) = self.repo.resolve()?;
        self.repo.list(project_dir.as_deref(), &global_dir)
    }

    /// 指定 ID のメモリを取得（複数可）。見つからない ID はエラーで返す。
    pub fn get(&self, ids: &[String]) -> Result<Vec<MemoryEntry>, Error> {
        let (project_dir, global_dir) = self.repo.resolve()?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let e = self.repo.get(project_dir.as_deref(), &global_dir, id)?;
            out.push(e);
        }
        Ok(out)
    }

    /// 指定 ID のメモリを削除（複数可）。見つからない ID はエラーで返す。
    pub fn remove(&self, ids: &[String]) -> Result<(), Error> {
        let (project_dir, global_dir) = self.repo.resolve()?;
        for id in ids {
            self.repo.remove(project_dir.as_deref(), &global_dir, id)?;
        }
        Ok(())
    }
}
