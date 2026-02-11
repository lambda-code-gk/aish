//! sessions コマンドのユースケース
//!
//! セッションルート配下のセッション一覧を列挙する。

use common::error::Error;
use common::ports::outbound::{EnvResolver, FileSystem, PathResolver, PathResolverInput};
use std::cmp::Reverse;
use std::path::PathBuf;
use std::sync::Arc;

/// sessions コマンドのユースケース
pub struct SessionsUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
    env_resolver: Arc<dyn EnvResolver>,
}

impl SessionsUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
        env_resolver: Arc<dyn EnvResolver>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
            env_resolver,
        }
    }

    /// セッション ID の一覧を新しい順（ID の降順）で返す
    pub fn list(&self, path_input: &PathResolverInput) -> Result<Vec<String>, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        // resolve_session_dir で生成されるパスから親ディレクトリを求めることで、
        // 実際に利用されているセッションルート（AISH_HOME/state/session または XDG_STATE_HOME/aish/session）
        // を特定する。
        let sample_session_dir = self
            .path_resolver
            .resolve_session_dir(path_input, &home_dir)?;
        let root = PathBuf::from(sample_session_dir)
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| Error::system("Failed to resolve sessions root directory"))?;

        if !self.fs.exists(&root) {
            return Ok(Vec::new());
        }

        // 現在のセッション ID（AISH_SESSION が指すディレクトリ名）を取得
        let current_id = self
            .env_resolver
            .session_dir_from_env()
            .and_then(|dir| dir.as_ref().file_name().map(|n| n.to_owned()))
            .and_then(|os_str| os_str.to_str().map(|s| s.to_string()));

        let mut ids = Vec::new();
        for entry in self.fs.read_dir(&root)? {
            if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
                // 「latest」などのエイリアスは除外し、ディレクトリのみを対象とする
                if name == "latest" {
                    continue;
                }
                if self
                    .fs
                    .metadata(&entry)
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
                {
                    let mut label = name.to_string();
                    if current_id.as_deref() == Some(name) {
                        label.push_str(" (current)");
                    }
                    ids.push(label);
                }
            }
        }

        // ID は時系列順に増加する形式なので、降順ソートで新しいものを先に表示する
        ids.sort_by_key(|s| Reverse(s.clone()));
        Ok(ids)
    }
}

