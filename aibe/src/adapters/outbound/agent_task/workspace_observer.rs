use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_OBSERVED_PATHS: usize = 4096;
const MAX_CHANGED_PATHS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    entries: BTreeMap<PathBuf, WorkspaceMetadataFingerprint>,
    pub incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceMetadataFingerprint {
    file_type: u32,
    is_dir: bool,
    is_symlink: bool,
    len: u64,
    dev: u64,
    ino: u64,
    mtime: i64,
    mtime_nsec: i64,
    ctime: i64,
    ctime_nsec: i64,
}

impl WorkspaceMetadataFingerprint {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            file_type: metadata.mode() & 0o170000,
            is_dir: metadata.is_dir(),
            is_symlink: metadata.file_type().is_symlink(),
            len: metadata.len(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
        }
    }
}

pub fn snapshot_workspace(root: &Path) -> WorkspaceSnapshot {
    let mut snapshot = WorkspaceSnapshot {
        entries: BTreeMap::new(),
        incomplete: false,
    };
    walk(root, root, &mut snapshot);
    snapshot
}

fn walk(root: &Path, dir: &Path, snapshot: &mut WorkspaceSnapshot) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        snapshot.incomplete = true;
        return;
    };
    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                snapshot.incomplete = true;
                continue;
            }
        };
        if snapshot.entries.len() >= MAX_OBSERVED_PATHS {
            snapshot.incomplete = true;
            return;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            snapshot.incomplete = true;
            continue;
        };
        // ctime の subsecond 値を観測できない regular file では、同一 size / mtime
        // rewrite を metadata だけで否定できない。値がちょうど秒境界だった場合も
        // false negative より保守的な incomplete を選ぶ。
        if metadata.is_file() && metadata.ctime_nsec() == 0 {
            snapshot.incomplete = true;
        }
        let relative = match path.strip_prefix(root) {
            Ok(path) => path.to_path_buf(),
            Err(_) => {
                snapshot.incomplete = true;
                continue;
            }
        };
        snapshot.entries.insert(
            relative,
            WorkspaceMetadataFingerprint::from_metadata(&metadata),
        );
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            walk(root, &path, snapshot);
        }
    }
}

pub fn observe_changes(
    before: &WorkspaceSnapshot,
    after: &WorkspaceSnapshot,
) -> (Vec<PathBuf>, bool) {
    let mut paths = before
        .entries
        .keys()
        .chain(after.entries.keys())
        .filter(|path| before.entries.get(*path) != after.entries.get(*path))
        .cloned()
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let incomplete = before.incomplete || after.incomplete || paths.len() > MAX_CHANGED_PATHS;
    paths.truncate(MAX_CHANGED_PATHS);
    (paths, incomplete)
}
