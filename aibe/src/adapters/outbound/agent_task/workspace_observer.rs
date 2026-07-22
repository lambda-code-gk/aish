use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_OBSERVED_PATHS: usize = 4096;
const MAX_CHANGED_PATHS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    entries: BTreeMap<PathBuf, (u64, Option<std::time::SystemTime>, bool)>,
    pub incomplete: bool,
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
    for entry in read_dir.flatten() {
        if snapshot.entries.len() >= MAX_OBSERVED_PATHS {
            snapshot.incomplete = true;
            return;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            snapshot.incomplete = true;
            continue;
        };
        let relative = match path.strip_prefix(root) {
            Ok(path) => path.to_path_buf(),
            Err(_) => continue,
        };
        snapshot.entries.insert(
            relative,
            (metadata.len(), metadata.modified().ok(), metadata.is_dir()),
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
