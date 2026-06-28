//! Work state と永続化不変条件。

use std::collections::{HashMap, HashSet};

use aibe_protocol::{
    validate_work_text, WorkEntryDto, WorkEntryKindDto, WorkItemDto, WorkSnapshotDto,
    WorkStatusDto, WORK_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Active,
    Paused,
    Deferred,
    Done,
    Abandoned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkEntryKind {
    Note,
    Idea,
    Decision,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    pub id: u64,
    pub title: String,
    pub goal: String,
    pub status: WorkStatus,
    pub parent_id: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub focus: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkEntry {
    pub id: u64,
    pub work_id: u64,
    pub kind: WorkEntryKind,
    pub text: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkState {
    pub schema_version: u32,
    pub revision: u64,
    pub next_work_id: u64,
    pub active_work_id: Option<u64>,
    pub stack: Vec<u64>,
    pub works: Vec<WorkItem>,
    pub entries: Vec<WorkEntry>,
}

impl Default for WorkState {
    fn default() -> Self {
        Self {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 0,
            next_work_id: 1,
            active_work_id: None,
            stack: Vec::new(),
            works: Vec::new(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkStateError {
    #[error("unsupported work schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("invalid work state: {0}")]
    Invalid(String),
}

impl WorkState {
    pub fn allocate_work_id(&mut self) -> Result<u64, WorkStateError> {
        let id = self.next_work_id;
        if id == 0 {
            return Err(WorkStateError::Invalid("work id overflow".into()));
        }
        self.next_work_id = id
            .checked_add(1)
            .ok_or_else(|| WorkStateError::Invalid("work id overflow".into()))?;
        Ok(id)
    }

    pub fn validate(&self) -> Result<(), WorkStateError> {
        if self.schema_version != WORK_SCHEMA_VERSION {
            return Err(WorkStateError::UnsupportedSchema(self.schema_version));
        }
        let mut ids = HashSet::new();
        let mut by_id = HashMap::new();
        let mut active = Vec::new();
        for work in &self.works {
            if work.id == 0 || !ids.insert(work.id) {
                return Err(WorkStateError::Invalid("duplicate or zero work id".into()));
            }
            for text in [
                Some(work.title.as_str()),
                Some(work.goal.as_str()),
                work.focus.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                validate_work_text(text).map_err(|error| {
                    WorkStateError::Invalid(format!("invalid work text: {error}"))
                })?;
            }
            if let Some(summary) = work.summary.as_deref() {
                validate_work_text(summary).map_err(|error| {
                    WorkStateError::Invalid(format!("invalid work summary: {error}"))
                })?;
            }
            if work.status == WorkStatus::Active {
                active.push(work.id);
            }
            by_id.insert(work.id, work);
        }
        for work in &self.works {
            if let Some(parent_id) = work.parent_id {
                if parent_id == 0 || parent_id == work.id || !by_id.contains_key(&parent_id) {
                    return Err(WorkStateError::Invalid(
                        "parent_id must reference a different existing work".into(),
                    ));
                }
            }
            let mut ancestors = HashSet::new();
            let mut current = Some(work.id);
            while let Some(id) = current {
                if !ancestors.insert(id) {
                    return Err(WorkStateError::Invalid(
                        "parent chain contains a cycle".into(),
                    ));
                }
                current = by_id.get(&id).and_then(|item| item.parent_id);
            }
        }
        if active.len() > 1 {
            return Err(WorkStateError::Invalid("multiple active works".into()));
        }
        if active.first().copied() != self.active_work_id {
            return Err(WorkStateError::Invalid(
                "active_work_id does not match active status".into(),
            ));
        }
        if self.stack.is_empty() && self.active_work_id.is_none() {
            // Empty state and finished root state are valid.
        } else if !self.stack.is_empty() && self.active_work_id.is_none() {
            return Err(WorkStateError::Invalid("stack requires active work".into()));
        }

        let mut stack_ids = HashSet::new();
        for id in &self.stack {
            let work = by_id
                .get(id)
                .ok_or_else(|| WorkStateError::Invalid("stack references missing work".into()))?;
            if !stack_ids.insert(*id) || work.status != WorkStatus::Paused {
                return Err(WorkStateError::Invalid(
                    "stack work must be unique and paused".into(),
                ));
            }
        }
        if let Some(active_id) = self.active_work_id {
            let mut expected_parent = by_id.get(&active_id).and_then(|work| work.parent_id);
            for parent_id in self.stack.iter().rev() {
                if expected_parent != Some(*parent_id) {
                    return Err(WorkStateError::Invalid(
                        "stack does not match parent chain".into(),
                    ));
                }
                expected_parent = by_id.get(parent_id).and_then(|work| work.parent_id);
            }
            if expected_parent.is_some() {
                return Err(WorkStateError::Invalid(
                    "stack is missing an ancestor".into(),
                ));
            }
        }
        let mut entry_ids = HashSet::new();
        for entry in &self.entries {
            if entry.id == 0 || !entry_ids.insert(entry.id) || !by_id.contains_key(&entry.work_id) {
                return Err(WorkStateError::Invalid(
                    "entry has duplicate id or missing work".into(),
                ));
            }
            validate_work_text(&entry.text).map_err(|error| {
                WorkStateError::Invalid(format!("invalid work entry text: {error}"))
            })?;
        }
        let max_work_id = self.works.iter().map(|work| work.id).max().unwrap_or(0);
        if self.next_work_id <= max_work_id {
            return Err(WorkStateError::Invalid(
                "next_work_id must be greater than stored ids".into(),
            ));
        }
        Ok(())
    }

    pub fn to_snapshot_dto(&self) -> WorkSnapshotDto {
        WorkSnapshotDto {
            revision: self.revision,
            active_work_id: self.active_work_id,
            stack: self.stack.clone(),
            works: self.works.iter().map(WorkItemDto::from).collect(),
            entries: self.entries.iter().map(WorkEntryDto::from).collect(),
        }
    }
}

impl From<WorkStatus> for WorkStatusDto {
    fn from(value: WorkStatus) -> Self {
        match value {
            WorkStatus::Active => Self::Active,
            WorkStatus::Paused => Self::Paused,
            WorkStatus::Deferred => Self::Deferred,
            WorkStatus::Done => Self::Done,
            WorkStatus::Abandoned => Self::Abandoned,
        }
    }
}

impl From<WorkEntryKind> for WorkEntryKindDto {
    fn from(value: WorkEntryKind) -> Self {
        match value {
            WorkEntryKind::Note => Self::Note,
            WorkEntryKind::Idea => Self::Idea,
            WorkEntryKind::Decision => Self::Decision,
        }
    }
}

impl From<&WorkItem> for WorkItemDto {
    fn from(value: &WorkItem) -> Self {
        Self {
            id: value.id,
            title: value.title.clone(),
            goal: value.goal.clone(),
            status: value.status.into(),
            parent_id: value.parent_id,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            finished_at_ms: value.finished_at_ms,
            focus: value.focus.clone(),
            summary: value.summary.clone(),
        }
    }
}

impl From<&WorkEntry> for WorkEntryDto {
    fn from(value: &WorkEntry) -> Self {
        Self {
            id: value.id,
            work_id: value.work_id,
            kind: value.kind.into(),
            text: value.text.clone(),
            created_at_ms: value.created_at_ms,
        }
    }
}
