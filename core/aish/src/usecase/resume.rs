//! resume コマンドのユースケース
//!
//! `aish resume [<id>]` で既存セッションを再開する。

use crate::ports::outbound::ShellRunner;
use common::error::Error;
use common::ports::outbound::{FileSystem, PathResolver, PathResolverInput};
use common::session::Session;
use std::cmp::Reverse;
use std::path::PathBuf;
use std::sync::Arc;

/// resume コマンドのユースケース
pub struct ResumeUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
    shell_runner: Arc<dyn ShellRunner>,
}

impl ResumeUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
        shell_runner: Arc<dyn ShellRunner>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
            shell_runner,
        }
    }

    /// セッションを再開する
    ///
    /// - `id` が Some の場合: その ID のセッションを再開
    /// - `id` が None の場合: 最新のセッション（ID 降順の先頭）を再開
    pub fn run(
        &self,
        path_input: &PathResolverInput,
        id: Option<&str>,
    ) -> Result<i32, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;

        // sessions と同様に、resolve_session_dir からルートディレクトリを特定
        let sample_session_dir = self
            .path_resolver
            .resolve_session_dir(path_input, &home_dir)?;
        let root = PathBuf::from(sample_session_dir)
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| Error::system("Failed to resolve sessions root directory"))?;

        if !self.fs.exists(&root) {
            return Err(Error::invalid_argument(
                "No sessions directory found to resume from.".to_string(),
            ));
        }

        // 対象セッションディレクトリを決定
        let target_dir = if let Some(id) = id {
            let candidate = root.join(id);
            if !self.fs.exists(&candidate)
                || !self
                    .fs
                    .metadata(&candidate)
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
            {
                return Err(Error::invalid_argument(format!(
                    "Session '{}' not found.",
                    id
                )));
            }
            candidate
        } else {
            // 最新（ID 降順の先頭）のディレクトリを選ぶ
            let mut ids = Vec::new();
            for entry in self.fs.read_dir(&root)? {
                if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
                    if name == "latest" {
                        continue;
                    }
                    if self
                        .fs
                        .metadata(&entry)
                        .map(|m| m.is_dir())
                        .unwrap_or(false)
                    {
                        ids.push(name.to_string());
                    }
                }
            }
            if ids.is_empty() {
                return Err(Error::invalid_argument(
                    "No sessions found to resume.".to_string(),
                ));
            }
            ids.sort_by_key(|s| Reverse(s.clone()));
            root.join(&ids[0])
        };

        // Session を構築して Shell を起動
        let session = Session::new(&target_dir, &home_dir)?;
        self.shell_runner.run(
            session.session_dir().as_ref(),
            session.aish_home().as_ref(),
        )
    }
}

