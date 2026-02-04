//! Shell コマンドのユースケース

use crate::cli::Config;
use crate::ports::outbound::ShellRunner;
use crate::wiring::App;
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

    /// App から ShellUseCase を作成する
    pub fn from_app(app: &App) -> Self {
        Self::new(
            Arc::clone(&app.path_resolver),
            Arc::clone(&app.shell_runner),
        )
    }

    /// Shell を実行する
    pub fn run(&self, config: &Config) -> Result<i32, Error> {
        let session = self.resolve_session(config)?;
        self.shell_runner.run(
            session.session_dir().as_ref(),
            session.aish_home().as_ref(),
        )
    }

    fn resolve_session(&self, config: &Config) -> Result<Session, Error> {
        let path_input = PathResolverInput {
            home_dir: config.home_dir.clone(),
            session_dir: config.session_dir.clone(),
        };
        let home_dir = self.path_resolver.resolve_home_dir(&path_input)?;
        let session_path = self.path_resolver.resolve_session_dir(&path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }
}
