//! 初期設定の展開（aish init）
//!
//! テンプレ（assets/defaults または AISH_DEFAULTS_DIR）を XDG/AISH_HOME の config にコピーする。

use common::error::Error;
use common::ports::outbound::{EnvResolver, FileSystem};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 初期設定展開の入力
#[derive(Debug, Clone)]
pub struct InitInput {
    /// テンプレートのルート（例: assets/defaults）。この config/ 以下がコピーされる。
    pub defaults_dir: PathBuf,
    pub force: bool,
    pub dry_run: bool,
}

/// 初期設定展開の結果（main で表示用）
#[derive(Debug, Clone)]
pub struct InitResult {
    pub config_dir: PathBuf,
    pub dry_run: bool,
    pub copied_count: u32,
    pub copied_paths: Vec<PathBuf>,
}

/// 初期設定展開ユースケース
pub struct InitUseCase {
    env_resolver: Arc<dyn EnvResolver>,
    fs: Arc<dyn FileSystem>,
}

impl InitUseCase {
    pub fn new(env_resolver: Arc<dyn EnvResolver>, fs: Arc<dyn FileSystem>) -> Self {
        Self { env_resolver, fs }
    }

    /// テンプレを config_dir にコピーする。表示は main の責務。
    pub fn run(&self, input: &InitInput) -> Result<InitResult, Error> {
        let dirs = self.env_resolver.resolve_dirs()?;
        let config_dir = dirs.config_dir.clone();
        let template_config = input.defaults_dir.join("config");
        if !self.fs.exists(template_config.as_path()) {
            return Err(Error::invalid_argument(format!(
                "Defaults config dir not found: {}",
                template_config.display()
            )));
        }
        let mut copied_paths = Vec::new();
        self.copy_tree(
            &template_config,
            &config_dir,
            Path::new(""),
            input.force,
            input.dry_run,
            &mut copied_paths,
        )?;
        let copied_count = copied_paths.len() as u32;
        Ok(InitResult {
            config_dir,
            dry_run: input.dry_run,
            copied_count,
            copied_paths,
        })
    }

    fn copy_tree(
        &self,
        src_root: &Path,
        dest_root: &Path,
        relative: &Path,
        force: bool,
        dry_run: bool,
        copied_paths: &mut Vec<PathBuf>,
    ) -> Result<(), Error> {
        let src = src_root.join(relative);
        let entries = self.fs.read_dir(&src)?;
        for entry in entries {
            let name = entry
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| Error::io_msg("non-UTF8 path".to_string()))?;
            let rel = if relative.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                relative.join(name)
            };
            let src_path = src_root.join(&rel);
            let dest_path = dest_root.join(&rel);
            let meta = self.fs.metadata(&src_path)?;
            if meta.is_dir() {
                if !dry_run {
                    let _ = self.fs.create_dir_all(&dest_path);
                }
                self.copy_tree(src_root, dest_root, &rel, force, dry_run, copied_paths)?;
            } else if meta.is_file() {
                if self.fs.exists(&dest_path) && !force {
                    continue;
                }
                if dry_run {
                    copied_paths.push(dest_path.clone());
                } else {
                    self.fs.create_dir_all(dest_path.parent().unwrap_or(dest_root))?;
                    let contents = self.fs.read_to_string(&src_path)?;
                    self.fs.write(&dest_path, &contents)?;
                    copied_paths.push(dest_path.clone());
                }
            }
        }
        Ok(())
    }
}
