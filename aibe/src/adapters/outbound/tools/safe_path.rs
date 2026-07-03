//! 読み取り / 書き込み用の共通パス解決（設計 §12.2）。

use std::os::unix::fs::FileTypeExt;
use std::path::{Component, Path, PathBuf};

use crate::ports::outbound::{FileWriteConfig, ReadFileConfig, ToolExecutionContext};

/// パス解決エラー（設計 §21 の語彙）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafePathError {
    pub code: &'static str,
    pub message: String,
}

impl SafePathError {
    fn path_not_allowed(message: impl Into<String>) -> Self {
        Self {
            code: "path_not_allowed",
            message: message.into(),
        }
    }

    fn symlink_not_allowed(message: impl Into<String>) -> Self {
        Self {
            code: "symlink_not_allowed",
            message: message.into(),
        }
    }

    fn unsupported_file_type(message: impl Into<String>) -> Self {
        Self {
            code: "unsupported_file_type",
            message: message.into(),
        }
    }
}

/// `read_file` 用パスポリシー。
#[derive(Debug, Clone)]
pub struct ReadPathPolicy {
    allowed_roots: Vec<PathBuf>,
}

impl ReadPathPolicy {
    pub fn from_config(config: &ReadFileConfig) -> Self {
        Self {
            allowed_roots: config.allowed_roots.clone(),
        }
    }

    pub fn new(allowed_roots: Vec<PathBuf>) -> Self {
        Self { allowed_roots }
    }

    pub fn allowed_roots(&self) -> &[PathBuf] {
        &self.allowed_roots
    }

    pub fn resolve_allowed_roots(&self, ctx: &ToolExecutionContext) -> Vec<PathBuf> {
        resolve_roots(&self.allowed_roots, ctx.base_dir())
    }

    pub fn validate_path_string(path_str: &str) -> Result<PathBuf, SafePathError> {
        validate_path_string(path_str, PathKind::Read)
    }

    pub async fn resolve_read_path(
        &self,
        ctx: &ToolExecutionContext,
        path: &Path,
    ) -> Result<PathBuf, SafePathError> {
        let resolved = resolve_against_base(path, ctx);
        let roots = self.resolve_allowed_roots(ctx);
        let canonical = tokio::fs::canonicalize(&resolved)
            .await
            .map_err(|e| SafePathError::path_not_allowed(e.to_string()))?;
        if !is_under_allowed_roots(&canonical, &roots) {
            return Err(SafePathError::path_not_allowed(
                "path is outside allowed_roots",
            ));
        }
        Ok(canonical)
    }
}

/// `write_file` / `apply_patch` 用パスポリシー（read とは別 `allowed_roots`）。
#[derive(Debug, Clone)]
pub struct WritePathPolicy {
    allowed_roots: Vec<PathBuf>,
}

impl WritePathPolicy {
    pub fn from_config(config: &FileWriteConfig) -> Self {
        Self {
            allowed_roots: config.allowed_roots.clone(),
        }
    }

    pub fn new(allowed_roots: Vec<PathBuf>) -> Self {
        Self { allowed_roots }
    }

    pub fn allowed_roots(&self) -> &[PathBuf] {
        &self.allowed_roots
    }

    pub fn resolve_allowed_roots(&self, ctx: &ToolExecutionContext) -> Vec<PathBuf> {
        resolve_roots(&self.allowed_roots, ctx.base_dir())
    }

    pub fn validate_path_string(path_str: &str) -> Result<PathBuf, SafePathError> {
        validate_path_string(path_str, PathKind::Write)
    }

    pub async fn resolve_write_path(
        &self,
        ctx: &ToolExecutionContext,
        path: &Path,
    ) -> Result<PathBuf, SafePathError> {
        let resolved = resolve_against_base(path, ctx);
        reject_symlinks_along_path(&resolved).await?;
        let roots = self.resolve_allowed_roots(ctx);
        let canonical = canonicalize_for_policy(&resolved).await?;
        if let Ok(meta) = tokio::fs::symlink_metadata(&resolved).await {
            if meta.file_type().is_symlink() {
                return Err(SafePathError::symlink_not_allowed(
                    "target path is a symlink",
                ));
            }
            if is_unsupported_file_type(&meta) {
                return Err(SafePathError::unsupported_file_type(
                    "special files are not supported",
                ));
            }
        }
        if !is_under_allowed_roots(&canonical, &roots) {
            return Err(SafePathError::path_not_allowed(
                "path is outside allowed_roots",
            ));
        }
        Ok(canonical)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathKind {
    Read,
    Write,
}

fn validate_path_string(path_str: &str, kind: PathKind) -> Result<PathBuf, SafePathError> {
    if path_str.trim().is_empty() {
        return Err(SafePathError::path_not_allowed("path must not be empty"));
    }
    if path_str.contains('\0') {
        return Err(SafePathError::path_not_allowed("path must not contain NUL"));
    }
    let path = PathBuf::from(path_str);
    if kind == PathKind::Write && path.is_absolute() {
        return Err(SafePathError::path_not_allowed(
            "path must be relative to client cwd",
        ));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(SafePathError::path_not_allowed(
            "path must not contain '..'".to_string(),
        ));
    }
    Ok(path)
}

fn resolve_against_base(path: &Path, ctx: &ToolExecutionContext) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.resolve_path(path)
    }
}

fn expand_home_path(path: &Path) -> PathBuf {
    if let Some(s) = path.to_str() {
        if let Some(rest) = s.strip_prefix("~/") {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            return PathBuf::from(home).join(rest);
        }
        if s == "~" {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            return PathBuf::from(home);
        }
    }
    path.to_path_buf()
}

fn resolve_roots(allowed_roots: &[PathBuf], base_dir: &Path) -> Vec<PathBuf> {
    allowed_roots
        .iter()
        .map(|p| {
            let root = if p == Path::new(".") {
                base_dir.to_path_buf()
            } else if p.is_absolute() {
                p.clone()
            } else {
                base_dir.join(p)
            };
            let expanded = if p.to_str().is_some_and(|s| s.starts_with('~')) {
                expand_home_path(p)
            } else {
                root
            };
            expanded.canonicalize().unwrap_or(expanded)
        })
        .collect()
}

fn is_under_allowed_roots(canonical: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| canonical.starts_with(root))
}

async fn canonicalize_for_policy(path: &Path) -> Result<PathBuf, SafePathError> {
    if tokio::fs::try_exists(path)
        .await
        .map_err(|e| SafePathError::path_not_allowed(e.to_string()))?
    {
        return tokio::fs::canonicalize(path)
            .await
            .map_err(|e| SafePathError::path_not_allowed(e.to_string()));
    }

    let mut probe = path.to_path_buf();
    while !tokio::fs::try_exists(&probe)
        .await
        .map_err(|e| SafePathError::path_not_allowed(e.to_string()))?
    {
        let Some(parent) = probe.parent() else {
            return Err(SafePathError::path_not_allowed(
                "path has no existing ancestor",
            ));
        };
        if parent.as_os_str().is_empty() {
            return Err(SafePathError::path_not_allowed(
                "path has no existing ancestor",
            ));
        }
        probe = parent.to_path_buf();
    }

    let canonical_base = tokio::fs::canonicalize(&probe)
        .await
        .map_err(|e| SafePathError::path_not_allowed(e.to_string()))?;
    let rel = path
        .strip_prefix(&probe)
        .map_err(|_| SafePathError::path_not_allowed("failed to resolve path prefix"))?;
    Ok(canonical_base.join(rel))
}

async fn reject_symlinks_along_path(path: &Path) -> Result<(), SafePathError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component);
            }
            Component::CurDir => {}
            Component::Normal(name) => {
                current.push(name);
                match tokio::fs::symlink_metadata(&current).await {
                    Ok(meta) => {
                        if meta.file_type().is_symlink() {
                            return Err(SafePathError::symlink_not_allowed(
                                "path component is a symlink",
                            ));
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // create 先の最終コンポーネントは未作成のためここで打ち切る。
                        break;
                    }
                    Err(e) => {
                        return Err(SafePathError::path_not_allowed(e.to_string()));
                    }
                }
            }
            Component::ParentDir => {}
        }
    }
    Ok(())
}

fn is_unsupported_file_type(meta: &std::fs::Metadata) -> bool {
    let ft = meta.file_type();
    ft.is_fifo() || ft.is_socket() || ft.is_block_device() || ft.is_char_device()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ClientCwd;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn ctx_at(path: &Path) -> ToolExecutionContext {
        ToolExecutionContext::new(ClientCwd::new(path.to_path_buf()).expect("absolute cwd"))
    }

    #[test]
    fn rejects_parent_traversal() {
        let err = ReadPathPolicy::validate_path_string("../outside").unwrap_err();
        assert_eq!(err.code, "path_not_allowed");
        assert!(err.message.contains("'..'"));
    }

    #[test]
    fn read_allows_absolute_path_string() {
        ReadPathPolicy::validate_path_string("/tmp/example.txt").expect("absolute read path");
    }

    #[test]
    fn write_rejects_absolute_path_string() {
        let err = WritePathPolicy::validate_path_string("/tmp/example.txt").unwrap_err();
        assert_eq!(err.code, "path_not_allowed");
    }

    #[tokio::test]
    async fn write_path_resolves_under_allowed_roots() {
        let dir = tempdir().expect("tempdir");
        let write_root = dir.path().join("writable");
        std::fs::create_dir_all(&write_root).expect("mkdir");
        std::fs::write(write_root.join("note.txt"), "ok").expect("write");

        let policy = WritePathPolicy::new(vec![write_root.clone()]);
        let ctx = ctx_at(dir.path());
        let got = policy
            .resolve_write_path(&ctx, Path::new("writable/note.txt"))
            .await
            .expect("resolve");
        let expected = write_root.join("note.txt").canonicalize().expect("canon");
        assert_eq!(got, expected);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_path_rejects_symlinks() {
        let base = tempdir().expect("base");
        let outside = tempdir().expect("outside");
        std::fs::write(outside.path().join("secret.txt"), "secret").expect("write");
        symlink(outside.path(), base.path().join("link")).expect("symlink");

        let policy = WritePathPolicy::new(vec![base.path().to_path_buf()]);
        let ctx = ctx_at(base.path());
        let err = policy
            .resolve_write_path(&ctx, Path::new("link/secret.txt"))
            .await
            .expect_err("symlink");
        assert_eq!(err.code, "symlink_not_allowed");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_path_rejects_special_files() {
        let dir = tempdir().expect("tempdir");
        let fifo = dir.path().join("pipe");
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");

        let policy = WritePathPolicy::new(vec![dir.path().to_path_buf()]);
        let ctx = ctx_at(dir.path());
        let err = policy
            .resolve_write_path(&ctx, Path::new("pipe"))
            .await
            .expect_err("fifo");
        assert_eq!(err.code, "unsupported_file_type");
    }
}
