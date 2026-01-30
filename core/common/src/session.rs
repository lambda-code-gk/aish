//! セッション管理
//!
//! セッションディレクトリの初期化と管理を提供します。

use crate::domain::{HomeDir, SessionDir};
use crate::error::io_error;
use std::path::PathBuf;

/// 検証済みパス管理構造体（内部実装）
struct ValidatedPath {
    path: PathBuf,
}

impl ValidatedPath {
    /// 新しい検証済みパスを作成
    ///
    /// # Arguments
    /// * `path` - ディレクトリのパス
    /// * `path_name` - エラーメッセージ用のパス名（例: "session directory", "home directory"）
    /// * `missing_handler` - パスが存在しない場合の処理を行うクロージャ
    ///
    /// # Returns
    /// 検証済みパス管理構造体、またはエラー
    ///
    /// # Errors
    /// ディレクトリの作成や正規化に失敗した場合、エラーを返します。
    fn new<F>(
        path: impl Into<PathBuf>,
        path_name: &str,
        missing_handler: F,
    ) -> Result<Self, (String, i32)>
    where
        F: FnOnce(&PathBuf, &str) -> Result<(), (String, i32)>,
    {
        let path = path.into();
        
        // ディレクトリが存在しない場合の処理
        if !path.exists() {
            missing_handler(&path, path_name)?;
        } else if !path.is_dir() {
            return Err(io_error(
                &format!("{} '{}' exists but is not a directory", path_name, path.display()),
                74
            ));
        }
        
        // フルパスに展開
        let path = std::fs::canonicalize(&path)
            .map_err(|e| io_error(
                &format!("Failed to canonicalize {} '{}': {}", path_name, path.display(), e),
                74
            ))?;
        
        Ok(ValidatedPath { path })
    }
    
    /// パスを取得
    fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// セッションディレクトリパス管理構造体
pub struct SessionPath {
    inner: ValidatedPath,
}

impl SessionPath {
    /// 新しいセッションディレクトリパスを作成（存在しない場合は作成し、フルパスに展開）
    ///
    /// # Arguments
    /// * `path` - セッションディレクトリのパス
    ///
    /// # Returns
    /// セッションディレクトリパス管理構造体、またはエラー
    ///
    /// # Errors
    /// ディレクトリの作成や正規化に失敗した場合、エラーを返します。
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, (String, i32)> {
        let inner = ValidatedPath::new(
            path,
            "session directory",
            |path, path_name| {
                std::fs::create_dir_all(path)
                    .map_err(|e| io_error(
                        &format!("Failed to create {} '{}': {}", path_name, path.display(), e),
                        74
                    ))?;
                Ok(())
            },
        )?;
        Ok(SessionPath { inner })
    }
    
    /// パスを取得
    pub fn path(&self) -> &PathBuf {
        self.inner.path()
    }
}

/// ホームディレクトリパス管理構造体
pub struct HomePath {
    inner: ValidatedPath,
}

impl HomePath {
    /// 新しいホームディレクトリパスを作成（存在しない場合はエラー、フルパスに展開）
    ///
    /// # Arguments
    /// * `path` - ホームディレクトリのパス
    ///
    /// # Returns
    /// ホームディレクトリパス管理構造体、またはエラー
    ///
    /// # Errors
    /// ディレクトリが存在しない場合や正規化に失敗した場合、エラーを返します。
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, (String, i32)> {
        let inner = ValidatedPath::new(
            path,
            "home directory",
            |path, path_name| {
                Err(io_error(
                    &format!("{} '{}' does not exist", path_name, path.display()),
                    74
                ))
            },
        )?;
        Ok(HomePath { inner })
    }
    
    /// パスを取得
    pub fn path(&self) -> &PathBuf {
        self.inner.path()
    }
}

/// セッション管理構造体
pub struct Session {
    session_dir: SessionDir,
    home_dir: HomeDir,
}

impl Session {
    /// 新しいセッションを作成
    ///
    /// # Arguments
    /// * `session_dir` - セッションディレクトリのパス
    /// * `home_dir` - ホームディレクトリのパス
    ///
    /// # Returns
    /// セッション管理構造体、またはエラー
    ///
    /// # Errors
    /// ディレクトリの作成に失敗した場合、エラーを返します。
    pub fn new(session_dir: impl Into<PathBuf>, home_dir: impl Into<PathBuf>) -> Result<Self, (String, i32)> {
        // セッションディレクトリを作成（存在しない場合は作成）
        let session_path = SessionPath::new(session_dir)?;
        let session_dir = SessionDir::new(session_path.path().clone());
        
        // ホームディレクトリを検証（存在しない場合はエラー）
        let home_path = HomePath::new(home_dir.into())?;
        let home_dir = HomeDir::new(home_path.path().clone());

        Ok(Session { session_dir, home_dir })
    }
    
    /// セッションディレクトリを取得
    pub fn session_dir(&self) -> &SessionDir {
        &self.session_dir
    }
    
    /// AISH_HOME環境変数の値を取得
    ///
    /// ホームディレクトリのパスを返す。
    pub fn aish_home(&self) -> &HomeDir {
        &self.home_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_session_new_creates_directory() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_new");
        let home_path = temp_dir.join("aish_test_home_new");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path).unwrap();
        }
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        
        // ホームディレクトリを作成
        fs::create_dir_all(&home_path).unwrap();
        
        // セッションを作成
        let session = Session::new(&test_path, &home_path);
        assert!(session.is_ok());
        
        // ディレクトリが作成されたことを確認
        assert!(test_path.exists());
        assert!(test_path.is_dir());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
        fs::remove_dir_all(&home_path).unwrap();
    }
    
    #[test]
    fn test_session_new_with_existing_directory() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_existing");
        let home_path = temp_dir.join("aish_test_home_existing");
        
        // 事前にディレクトリを作成
        fs::create_dir_all(&test_path).unwrap();
        fs::create_dir_all(&home_path).unwrap();
        
        // セッションを作成（既存のディレクトリを使用）
        let session = Session::new(&test_path, &home_path);
        assert!(session.is_ok());
        
        // ディレクトリが存在することを確認
        assert!(test_path.exists());
        assert!(test_path.is_dir());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
        fs::remove_dir_all(&home_path).unwrap();
    }
    
    #[test]
    fn test_session_path() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_path");
        let home_path = temp_dir.join("aish_test_home_path");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path).unwrap();
        }
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        
        // ホームディレクトリを作成
        fs::create_dir_all(&home_path).unwrap();
        
        let session = Session::new(&test_path, &home_path).unwrap();
        assert_eq!(session.session_dir().as_ref(), test_path.as_path());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
        fs::remove_dir_all(&home_path).unwrap();
    }
    
    #[test]
    fn test_session_new_with_nested_path() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_nested").join("subdir").join("deep");
        let home_path = temp_dir.join("aish_test_home_nested");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path.parent().unwrap().parent().unwrap()).unwrap();
        }
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        
        // ホームディレクトリを作成
        fs::create_dir_all(&home_path).unwrap();
        
        // ネストされたパスでセッションを作成
        let session = Session::new(&test_path, &home_path);
        assert!(session.is_ok());
        
        // ディレクトリが作成されたことを確認
        assert!(test_path.exists());
        assert!(test_path.is_dir());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path.parent().unwrap().parent().unwrap()).unwrap();
        fs::remove_dir_all(&home_path).unwrap();
    }
    
    #[test]
    fn test_session_with_home_dir() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_home");
        let home_path = temp_dir.join("aish_test_home");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path).unwrap();
        }
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        
        // ホームディレクトリが存在しない場合はエラーを返す
        let session = Session::new(&test_path, &home_path);
        assert!(session.is_err());
        
        // ホームディレクトリを作成してから再度試行
        fs::create_dir_all(&home_path).unwrap();
        
        // ホームディレクトリが存在する場合は成功
        let session = Session::new(&test_path, &home_path);
        assert!(session.is_ok());
        
        let session = session.unwrap();
        // セッションディレクトリが作成されたことを確認
        assert!(test_path.exists());
        assert!(test_path.is_dir());
        // ホームディレクトリは既に存在していたことを確認
        assert!(home_path.exists());
        assert!(home_path.is_dir());
        
        // フルパスに展開されているため、canonicalizeしたパスと比較
        let canonical_home = std::fs::canonicalize(&home_path).unwrap();
        assert_eq!(session.aish_home().as_ref(), canonical_home.as_path());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
        fs::remove_dir_all(&home_path).unwrap();
    }
    
    #[test]
    fn test_session_aish_home() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_home2");
        let home_path = temp_dir.join("aish_test_home2");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path).unwrap();
        }
        if home_path.exists() {
            fs::remove_dir_all(&home_path).unwrap();
        }
        
        // ホームディレクトリを作成
        fs::create_dir_all(&home_path).unwrap();
        
        // セッションを作成
        let session = Session::new(&test_path, &home_path).unwrap();
        // フルパスに展開されているため、canonicalizeしたパスと比較
        let canonical_home = std::fs::canonicalize(&home_path).unwrap();
        assert_eq!(session.aish_home().as_ref(), canonical_home.as_path());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
        fs::remove_dir_all(&home_path).unwrap();
    }
}

#[cfg(test)]
mod session_path_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_session_path_new_creates_directory() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_path_new");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path).unwrap();
        }
        
        // セッションパスを作成（存在しない場合は作成）
        let path = SessionPath::new(&test_path);
        assert!(path.is_ok());
        
        // ディレクトリが作成されたことを確認
        assert!(test_path.exists());
        assert!(test_path.is_dir());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
    }
    
    #[test]
    fn test_session_path_new_with_existing_directory() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_path_existing");
        
        // 事前にディレクトリを作成
        fs::create_dir_all(&test_path).unwrap();
        
        // セッションパスを作成（既存のディレクトリを使用）
        let path = SessionPath::new(&test_path);
        assert!(path.is_ok());
        
        // ディレクトリが存在することを確認
        assert!(test_path.exists());
        assert!(test_path.is_dir());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
    }
    
    #[test]
    fn test_session_path_canonicalize() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_session_path_canonicalize");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path).unwrap();
        }
        
        // セッションパスを作成
        let path = SessionPath::new(&test_path).unwrap();
        let canonical_path = path.path();
        
        // フルパスになっていることを確認
        assert!(canonical_path.is_absolute());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
    }
}

#[cfg(test)]
mod home_path_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_home_path_new_fails_if_not_exists() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_home_path_not_exists");
        
        // テスト前にディレクトリが存在しないことを確認
        if test_path.exists() {
            fs::remove_dir_all(&test_path).unwrap();
        }
        
        // ホームパスを作成（存在しない場合はエラー）
        let path = HomePath::new(&test_path);
        assert!(path.is_err());
    }
    
    #[test]
    fn test_home_path_new_with_existing_directory() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_home_path_existing");
        
        // 事前にディレクトリを作成
        fs::create_dir_all(&test_path).unwrap();
        
        // ホームパスを作成（既存のディレクトリを使用）
        let path = HomePath::new(&test_path);
        assert!(path.is_ok());
        
        // ディレクトリが存在することを確認
        assert!(test_path.exists());
        assert!(test_path.is_dir());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
    }
    
    #[test]
    fn test_home_path_canonicalize() {
        let temp_dir = std::env::temp_dir();
        let test_path = temp_dir.join("aish_test_home_path_canonicalize");
        
        // 事前にディレクトリを作成
        fs::create_dir_all(&test_path).unwrap();
        
        // ホームパスを作成
        let path = HomePath::new(&test_path).unwrap();
        let canonical_path = path.path();
        
        // フルパスになっていることを確認
        assert!(canonical_path.is_absolute());
        
        // クリーンアップ
        fs::remove_dir_all(&test_path).unwrap();
    }
}

