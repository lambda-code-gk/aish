//! Clear コマンドのユースケース

use common::ports::outbound::FileSystem;
use common::error::Error;
use common::ports::outbound::{PathResolver, PathResolverInput};
use common::session::Session;
use std::path::Path;
use std::sync::Arc;

/// Clear コマンドのユースケース
pub struct ClearUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
}

impl ClearUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
        }
    }

    /// Clear を実行する
    ///
    /// `session_explicitly_specified` は、セッションが明示的に指定されているかを示す。
    /// CLI オプション (-s/-d) や環境変数 (AISH_SESSION/AISH_HOME) から main/cli 側で判定し、
    /// この引数として渡す（usecase は環境変数を直接参照しない）。
    pub fn run(
        &self,
        path_input: &PathResolverInput,
        session_explicitly_specified: bool,
    ) -> Result<i32, Error> {
        // clear コマンドはセッションが明示的に指定されている必要がある
        if !session_explicitly_specified {
            return Err(Error::invalid_argument(
                "The 'clear' command requires a session to be specified. \
                 Use -s/--session-dir, -d/--home-dir, or set AISH_SESSION environment variable.",
            ));
        }
        let session = self.resolve_session(path_input)?;
        self.clear_parts(session.session_dir().as_ref())
    }

    fn resolve_session(&self, path_input: &PathResolverInput) -> Result<Session, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self.path_resolver.resolve_session_dir(path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }

    fn clear_parts(&self, session_dir: &Path) -> Result<i32, Error> {
        if !self.fs.exists(session_dir) {
            return Ok(0);
        }

        // ディレクトリ内のファイル一覧を取得
        let entries = self.fs.read_dir(session_dir)?;

        // part_ で始まるファイルを削除
        for entry in entries {
            if let Some(file_name) = entry.file_name().and_then(|n| n.to_str()) {
                if file_name.starts_with("part_") {
                    // ファイルかどうか確認
                    if self.fs.metadata(&entry).map(|m| m.is_file()).unwrap_or(false) {
                        self.fs.remove_file(&entry)?;
                    }
                }
            }
        }

        Ok(0)
    }
}
