//! Mute コマンドのユースケース
//!
//! 以降の console.txt への記録を停止する。

use common::error::Error;
use common::ports::outbound::{FileSystem, PathResolver, PathResolverInput, Signal};
use common::session::Session;
use std::path::Path;
use std::sync::Arc;

/// Mute コマンドのユースケース
pub struct MuteUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
    signal: Arc<dyn Signal>,
}

impl MuteUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
        signal: Arc<dyn Signal>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
            signal,
        }
    }

    /// Mute を実行する
    pub fn run(&self, path_input: &PathResolverInput) -> Result<i32, Error> {
        let session = self.resolve_session(path_input)?;
        self.mute_console_log(session.session_dir().as_ref())
    }

    fn resolve_session(&self, path_input: &PathResolverInput) -> Result<Session, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self
            .path_resolver
            .resolve_session_dir(path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }

    fn mute_console_log(&self, session_dir: &Path) -> Result<i32, Error> {
        let pid_file_path = session_dir.join("AISH_PID");

        if !self.fs.exists(&pid_file_path) {
            // シェルプロセスが起動していない場合は何もしない
            return Ok(0);
        }

        // 以降の console.txt への記録を停止するためのフラグファイルを作成
        let mute_flag_path = session_dir.join("console.muted");
        self.fs.write(&mute_flag_path, "muted")?;

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::StdFileSystem;
    use common::ports::outbound::PathResolver;
    use common::ports::outbound::PathResolverInput;
    use common::ports::outbound::Signal as SignalPort;
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

    #[cfg(unix)]
    struct TestSignal;

    #[cfg(unix)]
    impl SignalPort for TestSignal {
        fn setup_sigwinch(&self) -> Result<(), Error> {
            Ok(())
        }

        fn setup_sigusr1(&self) -> Result<(), Error> {
            Ok(())
        }

        fn setup_sigusr2(&self) -> Result<(), Error> {
            Ok(())
        }

        fn check_sigwinch(&self) -> bool {
            false
        }

        fn check_sigusr1(&self) -> bool {
            false
        }

        fn check_sigusr2(&self) -> bool {
            false
        }

        fn send_signal(&self, pid: i32, sig: i32) -> Result<(), Error> {
            Err(Error::io_msg(format!(
                "unexpected signal send in test: pid={}, sig={}",
                pid, sig
            )))
        }
    }

    #[test]
    fn test_mute_creates_flag_when_pid_exists() {
        let temp_dir = std::path::PathBuf::from("/tmp").join("aish_test_mute_usecase");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        let home_dir = temp_dir.join("home");
        let session_dir = temp_dir.join("session");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(&session_dir).unwrap();

        // AISH_PID ファイルを作成（シェルが動いている想定）
        let pid_path = session_dir.join("AISH_PID");
        std::fs::write(&pid_path, "12345").unwrap();

        let path_resolver: Arc<dyn PathResolver> = Arc::new(TestPathResolver);
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);

        #[cfg(unix)]
        {
            let signal = Arc::new(TestSignal);
            let usecase = MuteUseCase::new(
                Arc::clone(&path_resolver),
                Arc::clone(&fs),
                Arc::clone(&signal) as Arc<dyn SignalPort>,
            );

            let input = PathResolverInput {
                home_dir: Some(path_to_string(&home_dir)),
                session_dir: Some(path_to_string(&session_dir)),
            };

            let result = usecase.run(&input);
            assert!(result.is_ok(), "mute run should succeed: {:?}", result.err());
            assert_eq!(result.unwrap(), 0);

            // フラグファイルが作成されていること（part ファイル・SIGUSR1 は送られない）
            let mute_flag = session_dir.join("console.muted");
            assert!(mute_flag.exists());
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_mute_is_noop_when_pid_missing() {
        let temp_dir = std::path::PathBuf::from("/tmp").join("aish_test_mute_no_pid");
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

        #[cfg(unix)]
        {
            let signal = Arc::new(TestSignal);
            let usecase = MuteUseCase::new(
                Arc::clone(&path_resolver),
                Arc::clone(&fs),
                Arc::clone(&signal) as Arc<dyn SignalPort>,
            );

            let input = PathResolverInput {
                home_dir: Some(path_to_string(&home_dir)),
                session_dir: Some(path_to_string(&session_dir)),
            };

            let result = usecase.run(&input);
            assert!(result.is_ok(), "mute run should succeed: {:?}", result.err());
            assert_eq!(result.unwrap(), 0);

            // フラグファイルも作成されない
            let mute_flag = session_dir.join("console.muted");
            assert!(!mute_flag.exists());
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    fn path_to_string(path: &PathBuf) -> String {
        path.to_string_lossy().to_string()
    }
}

