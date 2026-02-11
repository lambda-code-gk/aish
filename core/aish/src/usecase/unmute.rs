//! Unmute コマンドのユースケース
//!
//! console.txt への記録を再開する（mute 時に作成されたフラグファイルを削除する）。

use common::error::Error;
use common::ports::outbound::{FileSystem, PathResolver, PathResolverInput};
use common::session::Session;
use std::path::Path;
use std::sync::Arc;

/// Unmute コマンドのユースケース
pub struct UnmuteUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
}

impl UnmuteUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
        }
    }

    /// Unmute を実行する
    pub fn run(&self, path_input: &PathResolverInput) -> Result<i32, Error> {
        let session = self.resolve_session(path_input)?;
        self.unmute_console_log(session.session_dir().as_ref())
    }

    fn resolve_session(&self, path_input: &PathResolverInput) -> Result<Session, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self
            .path_resolver
            .resolve_session_dir(path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }

    fn unmute_console_log(&self, session_dir: &Path) -> Result<i32, Error> {
        let mute_flag_path = session_dir.join("console.muted");
        if self.fs.exists(&mute_flag_path) {
            self.fs.remove_file(&mute_flag_path)?;
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;
    use common::ports::outbound::PathResolver;
    use common::ports::outbound::PathResolverInput;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct TestPathResolver;

    impl PathResolver for TestPathResolver {
        fn resolve_home_dir(&self, input: &PathResolverInput) -> Result<String, Error> {
            input
                .home_dir
                .clone()
                .ok_or_else(|| Error::invalid_argument("home_dir is required in test".to_string()))
        }

        fn resolve_session_dir(
            &self,
            input: &PathResolverInput,
            _home_dir: &str,
        ) -> Result<String, Error> {
            input.session_dir.clone().ok_or_else(|| {
                Error::invalid_argument("session_dir is required in test".to_string())
            })
        }
    }

    #[test]
    fn test_unmute_removes_flag_if_exists() {
        let temp_dir = std::path::PathBuf::from("/tmp").join("aish_test_unmute_flag");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        let home_dir = temp_dir.join("home");
        let session_dir = temp_dir.join("session");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(&session_dir).unwrap();

        let mute_flag = session_dir.join("console.muted");
        std::fs::write(&mute_flag, "muted").unwrap();
        assert!(mute_flag.exists());

        let path_resolver: Arc<dyn PathResolver> = Arc::new(TestPathResolver);
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);

        let usecase = UnmuteUseCase::new(
            Arc::clone(&path_resolver),
            Arc::clone(&fs),
        );

        let input = PathResolverInput {
            home_dir: Some(path_to_string(&home_dir)),
            session_dir: Some(path_to_string(&session_dir)),
        };

        let result = usecase.run(&input);
        assert!(result.is_ok(), "unmute run should succeed: {:?}", result.err());
        assert_eq!(result.unwrap(), 0);

        assert!(!mute_flag.exists(), "mute flag should be removed");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_unmute_is_ok_when_flag_missing() {
        let temp_dir = std::path::PathBuf::from("/tmp").join("aish_test_unmute_no_flag");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        let home_dir = temp_dir.join("home");
        let session_dir = temp_dir.join("session");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(&session_dir).unwrap();

        let path_resolver: Arc<dyn PathResolver> = Arc::new(TestPathResolver);
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);

        let usecase = UnmuteUseCase::new(
            Arc::clone(&path_resolver),
            Arc::clone(&fs),
        );

        let input = PathResolverInput {
            home_dir: Some(path_to_string(&home_dir)),
            session_dir: Some(path_to_string(&session_dir)),
        };

        let result = usecase.run(&input);
        assert!(result.is_ok(), "unmute run should succeed: {:?}", result.err());
        assert_eq!(result.unwrap(), 0);

        let mute_flag = session_dir.join("console.muted");
        assert!(!mute_flag.exists(), "mute flag should still not exist");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    fn path_to_string(path: &PathBuf) -> String {
        path.to_string_lossy().to_string()
    }
}

