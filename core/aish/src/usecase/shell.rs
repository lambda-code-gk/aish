//! Shell コマンドのユースケース

use crate::ports::outbound::ShellRunner;
use common::error::Error;
use common::ports::outbound::{PathResolver, PathResolverInput};
use common::session::Session;
use std::sync::Arc;

/// Shell コマンドのユースケース
pub struct ShellUseCase {
    path_resolver: Arc<dyn PathResolver>,
    shell_runner: Arc<dyn ShellRunner>,
}

impl ShellUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        shell_runner: Arc<dyn ShellRunner>,
    ) -> Self {
        Self {
            path_resolver,
            shell_runner,
        }
    }

    /// Shell を実行する
    pub fn run(&self, path_input: &PathResolverInput) -> Result<i32, Error> {
        let session = self.resolve_session(path_input)?;
        self.shell_runner.run(
            session.session_dir().as_ref(),
            session.aish_home().as_ref(),
        )
    }

    fn resolve_session(&self, path_input: &PathResolverInput) -> Result<Session, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self.path_resolver.resolve_session_dir(path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }
}
