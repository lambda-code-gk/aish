//! プロファイル一覧取得アダプタ（common::llm の load_profiles_config / list_available_profiles を使用）

use std::sync::Arc;

use common::ports::outbound::{EnvResolver, FileSystem};

use crate::ports::outbound::ProfileLister;

/// 標準プロファイル一覧取得（profiles.json と common::llm を使用）
pub struct StdProfileLister {
    fs: Arc<dyn FileSystem>,
    env_resolver: Arc<dyn EnvResolver>,
}

impl StdProfileLister {
    pub fn new(fs: Arc<dyn FileSystem>, env_resolver: Arc<dyn EnvResolver>) -> Self {
        Self { fs, env_resolver }
    }
}

impl ProfileLister for StdProfileLister {
    fn list_profiles(&self) -> Result<(Vec<String>, Option<String>), common::error::Error> {
        let cfg_opt = common::llm::load_profiles_config(
            self.fs.as_ref(),
            self.env_resolver.as_ref(),
        )?;
        Ok(common::llm::list_available_profiles(cfg_opt.as_ref()))
    }
}
