//! manifest 優先の reviewed 履歴ローダ

use crate::adapter::session_manifest;
use crate::domain::{CompactionRecord, History, ManifestRecordV1, ManifestRole};
use crate::ports::outbound::SessionHistoryLoader;
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::FileSystem;
use common::safe_session_path::{is_safe_reviewed_path, is_safe_summary_basename, resolve_under_session_dir, REVIEWED_DIR};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// LLM に渡す履歴ビューを構築する strategy。
pub(crate) trait HistoryViewStrategy: Send + Sync {
    fn build_history(
        &self,
        fs: &dyn FileSystem,
        dir: &Path,
        load_max: usize,
    ) -> Result<History, Error>;
}

/// manifest + compaction を使う既定ビュー strategy。
pub(crate) struct ManifestTailCompactionViewStrategy;

impl HistoryViewStrategy for ManifestTailCompactionViewStrategy {
    fn build_history(
        &self,
        fs: &dyn FileSystem,
        dir: &Path,
        load_max: usize,
    ) -> Result<History, Error> {
        let records = session_manifest::load_all(fs, dir)?;
        let from_index = session_manifest::load_send_from_index(fs, dir);
        let range = records.get(from_index..).unwrap_or_default();
        let tail = session_manifest::tail_message_records(range, load_max);

        let mut history = History::new();
        let oldest_tail_id = tail
            .first()
            .and_then(|r| r.message())
            .map(|m| m.id.clone());

        if let Some(oldest_id) = oldest_tail_id {
            if let Some(comp) = latest_compaction_before(&records, &oldest_id) {
                if is_safe_summary_basename(&comp.summary_path) {
                    let summary_path = dir.join(&comp.summary_path);
                    if let Some(safe_path) = resolve_under_session_dir(dir, &summary_path) {
                        if let Ok(summary) = fs.read_to_string(&safe_path) {
                            history.push_assistant(summary);
                        }
                    }
                }
            }
        }

        for rec in tail {
            let Some(msg) = rec.message() else {
                continue;
            };
            if !is_safe_reviewed_path(&msg.reviewed_path) {
                continue;
            }
            let reviewed_path = dir.join(&msg.reviewed_path);
            let Some(safe_path) = resolve_under_session_dir(dir, &reviewed_path) else {
                continue;
            };
            match fs.read_to_string(&safe_path) {
                Ok(content) => match msg.role {
                    ManifestRole::User => history.push_user(content),
                    ManifestRole::Assistant => history.push_assistant(content),
                },
                Err(e) => eprintln!(
                    "Warning: Failed to read reviewed file '{}': {}",
                    safe_path.display(),
                    e
                ),
            }
        }
        Ok(history)
    }
}

/// manifest がない場合の reviewed ファイル走査 strategy。
pub(crate) struct ReviewedTailViewStrategy;

impl HistoryViewStrategy for ReviewedTailViewStrategy {
    fn build_history(
        &self,
        fs: &dyn FileSystem,
        dir: &Path,
        load_max: usize,
    ) -> Result<History, Error> {
        let reviewed_dir = dir.join(REVIEWED_DIR);
        if !fs.exists(&reviewed_dir) {
            return Ok(History::new());
        }
        let mut reviewed_files: Vec<PathBuf> = fs
            .read_dir(&reviewed_dir)?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|s| {
                        s.starts_with("reviewed_")
                            && (s.ends_with("_user.txt") || s.ends_with("_assistant.txt"))
                    })
                    && fs.metadata(path).map(|m| m.is_file()).unwrap_or(false)
            })
            .collect();
        reviewed_files.sort();
        if reviewed_files.len() > load_max {
            let keep_from = reviewed_files.len() - load_max;
            reviewed_files = reviewed_files.split_off(keep_from);
        }

        let mut history = History::new();
        for reviewed_file in reviewed_files {
            match fs.read_to_string(&reviewed_file) {
                Ok(content) => {
                    if let Some(name_str) = reviewed_file.file_name().and_then(|n| n.to_str()) {
                        if name_str.ends_with("_user.txt") {
                            history.push_user(content);
                        } else if name_str.ends_with("_assistant.txt") {
                            history.push_assistant(content);
                        }
                    }
                }
                Err(e) => eprintln!(
                    "Warning: Failed to read reviewed file '{}': {}",
                    reviewed_file.display(),
                    e
                ),
            }
        }
        Ok(history)
    }
}

pub struct ManifestReviewedSessionStorage {
    fs: Arc<dyn FileSystem>,
    load_max: usize,
    manifest_view_strategy: Arc<dyn HistoryViewStrategy>,
    fallback_view_strategy: Arc<dyn HistoryViewStrategy>,
}

impl ManifestReviewedSessionStorage {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(fs: Arc<dyn FileSystem>, load_max: usize) -> Self {
        Self::with_strategies(
            fs,
            load_max,
            Arc::new(ManifestTailCompactionViewStrategy),
            Arc::new(ReviewedTailViewStrategy),
        )
    }

    pub(crate) fn with_strategies(
        fs: Arc<dyn FileSystem>,
        load_max: usize,
        manifest_view_strategy: Arc<dyn HistoryViewStrategy>,
        fallback_view_strategy: Arc<dyn HistoryViewStrategy>,
    ) -> Self {
        Self {
            fs,
            load_max,
            manifest_view_strategy,
            fallback_view_strategy,
        }
    }
}

impl SessionHistoryLoader for ManifestReviewedSessionStorage {
    fn load(&self, session_dir: &SessionDir) -> Result<History, Error> {
        if !self.fs.exists(session_dir.as_ref()) {
            return Ok(History::new());
        }
        if self
            .fs
            .metadata(session_dir.as_ref())
            .map(|m| !m.is_dir())
            .unwrap_or(true)
        {
            return Ok(History::new());
        }
        let dir = session_dir.as_ref();
        let manifest_path = session_manifest::manifest_path(dir);
        if self.fs.exists(&manifest_path) {
            self.manifest_view_strategy
                .build_history(self.fs.as_ref(), dir, self.load_max)
        } else {
            self.fallback_view_strategy
                .build_history(self.fs.as_ref(), dir, self.load_max)
        }
    }
}

fn latest_compaction_before<'a>(
    records: &'a [ManifestRecordV1],
    oldest_tail_id: &str,
) -> Option<&'a CompactionRecord> {
    records
        .iter()
        .filter_map(|r| r.compaction())
        .filter(|c| c.to_id.as_str() < oldest_tail_id)
        .last()
}

