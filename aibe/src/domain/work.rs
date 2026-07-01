//! Work state と永続化不変条件。

use std::collections::{HashMap, HashSet};

use aibe_protocol::{
    validate_work_text, WorkEntryDto, WorkEntryKindDto, WorkInputError, WorkItemDto,
    WorkMutationKindDto, WorkMutationOutcomeDto, WorkOperationDto, WorkSnapshotDto, WorkStatusDto,
    WORK_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

use super::MemoryBlock;

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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkMutationError {
    #[error("work stack is not empty; use `ai work pop` first")]
    StackNotEmpty,
    #[error("no active work; start one with `ai work start <goal>`")]
    NoActiveWork,
    #[error("work stack is empty; no previous work to return to")]
    EmptyStack,
    #[error("work #{0} was not found")]
    WorkNotFound(u64),
    #[error("work #{0} is already done; reopen is not supported yet")]
    WorkAlreadyDone(u64),
    #[error("work #{0} is abandoned and cannot be switched")]
    WorkAbandoned(u64),
    #[error("work #{0} is not paused or deferred")]
    WorkNotSwitchable(u64),
    #[error("work operation is not supported")]
    UnsupportedOperation,
    #[error(transparent)]
    InvalidOperation(#[from] WorkInputError),
    #[error(transparent)]
    InvalidState(#[from] WorkStateError),
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

    pub fn apply(
        &mut self,
        operation: &WorkOperationDto,
        now_ms: u64,
    ) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        self.validate()?;
        operation.validate()?;
        match operation {
            WorkOperationDto::Start { goal } => self.start(goal.clone(), now_ms),
            WorkOperationDto::Focus { text } => self.focus(text.clone(), now_ms),
            WorkOperationDto::AddEntry { kind, text } => {
                self.add_entry((*kind).into(), text.clone(), now_ms)
            }
            WorkOperationDto::Defer { text } => self.defer(text.clone(), now_ms),
            WorkOperationDto::Switch { work_id } => self.switch(*work_id, now_ms),
            WorkOperationDto::Push { goal } => self.push(goal.clone(), now_ms),
            WorkOperationDto::Pop => self.pop(now_ms),
            WorkOperationDto::Finish => self.finish(now_ms),
        }
    }

    fn start(
        &mut self,
        goal: String,
        now_ms: u64,
    ) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        if !self.stack.is_empty() {
            return Err(WorkMutationError::StackNotEmpty);
        }
        let work_id = self.allocate_work_id()?;
        let previous_work_id = self.active_work_id;
        if let Some(active_id) = previous_work_id {
            let active = self
                .works
                .iter_mut()
                .find(|work| work.id == active_id)
                .ok_or_else(|| {
                    WorkStateError::Invalid("active work is missing during start".into())
                })?;
            active.status = WorkStatus::Paused;
            active.updated_at_ms = now_ms;
        }
        self.works.push(WorkItem {
            id: work_id,
            title: goal.clone(),
            goal,
            status: WorkStatus::Active,
            parent_id: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            finished_at_ms: None,
            focus: None,
            summary: None,
        });
        self.active_work_id = Some(work_id);
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Start,
            work_id: Some(work_id),
            previous_work_id,
        })
    }

    fn focus(
        &mut self,
        text: String,
        now_ms: u64,
    ) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        let active_id = self.active_work_id.ok_or(WorkMutationError::NoActiveWork)?;
        let active = self
            .works
            .iter_mut()
            .find(|work| work.id == active_id)
            .ok_or_else(|| WorkStateError::Invalid("active work is missing during focus".into()))?;
        active.focus = Some(text);
        active.updated_at_ms = now_ms;
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Focus,
            work_id: Some(active_id),
            previous_work_id: None,
        })
    }

    fn add_entry(
        &mut self,
        kind: WorkEntryKind,
        text: String,
        now_ms: u64,
    ) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        let active_id = self.active_work_id.ok_or(WorkMutationError::NoActiveWork)?;
        let entry_id = self
            .entries
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| WorkStateError::Invalid("work entry id overflow".into()))?;
        self.entries.push(WorkEntry {
            id: entry_id,
            work_id: active_id,
            kind,
            text,
            created_at_ms: now_ms,
        });
        if let Some(active) = self.works.iter_mut().find(|work| work.id == active_id) {
            active.updated_at_ms = now_ms;
        }
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::AddEntry,
            work_id: Some(active_id),
            previous_work_id: None,
        })
    }

    fn defer(
        &mut self,
        text: String,
        now_ms: u64,
    ) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        let work_id = self.allocate_work_id()?;
        self.works.push(WorkItem {
            id: work_id,
            title: text.clone(),
            goal: text,
            status: WorkStatus::Deferred,
            parent_id: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            finished_at_ms: None,
            focus: None,
            summary: None,
        });
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Defer,
            work_id: Some(work_id),
            previous_work_id: None,
        })
    }

    fn push(
        &mut self,
        goal: String,
        now_ms: u64,
    ) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        let previous_work_id = self.active_work_id.ok_or(WorkMutationError::NoActiveWork)?;
        let new_work_id = self.allocate_work_id()?;
        if let Some(previous) = self
            .works
            .iter_mut()
            .find(|work| work.id == previous_work_id)
        {
            previous.status = WorkStatus::Paused;
            previous.updated_at_ms = now_ms;
        } else {
            return Err(
                WorkStateError::Invalid("active work is missing during push".into()).into(),
            );
        }
        self.stack.push(previous_work_id);
        self.works.push(WorkItem {
            id: new_work_id,
            title: goal.clone(),
            goal,
            status: WorkStatus::Active,
            parent_id: Some(previous_work_id),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            finished_at_ms: None,
            focus: None,
            summary: None,
        });
        self.active_work_id = Some(new_work_id);
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Push,
            work_id: Some(new_work_id),
            previous_work_id: Some(previous_work_id),
        })
    }

    fn pop(&mut self, now_ms: u64) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        let active_id = self.active_work_id.ok_or(WorkMutationError::NoActiveWork)?;
        let Some(parent_id) = self.stack.last().copied() else {
            return Err(WorkMutationError::EmptyStack);
        };
        let active_index = self
            .works
            .iter()
            .position(|work| work.id == active_id)
            .ok_or_else(|| WorkStateError::Invalid("active work is missing during pop".into()))?;
        if self.works[active_index].parent_id != Some(parent_id) {
            return Err(
                WorkStateError::Invalid("stack does not match current parent".into()).into(),
            );
        }
        self.works[active_index].status = WorkStatus::Done;
        self.works[active_index].updated_at_ms = now_ms;
        self.works[active_index].finished_at_ms = Some(now_ms);
        self.stack.pop();
        let parent = self
            .works
            .iter_mut()
            .find(|work| work.id == parent_id)
            .ok_or_else(|| WorkStateError::Invalid("parent work is missing during pop".into()))?;
        parent.status = WorkStatus::Active;
        parent.updated_at_ms = now_ms;
        self.active_work_id = Some(parent_id);
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Pop,
            work_id: Some(active_id),
            previous_work_id: Some(parent_id),
        })
    }

    fn switch(
        &mut self,
        work_id: u64,
        now_ms: u64,
    ) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        if !self.stack.is_empty() {
            return Err(WorkMutationError::StackNotEmpty);
        }
        let target_index = self
            .works
            .iter()
            .position(|work| work.id == work_id)
            .ok_or(WorkMutationError::WorkNotFound(work_id))?;
        let target_status = self.works[target_index].status;
        match target_status {
            WorkStatus::Paused | WorkStatus::Deferred => {}
            WorkStatus::Done => return Err(WorkMutationError::WorkAlreadyDone(work_id)),
            WorkStatus::Abandoned => return Err(WorkMutationError::WorkAbandoned(work_id)),
            WorkStatus::Active if self.active_work_id == Some(work_id) => {
                return Ok(WorkMutationOutcomeDto {
                    kind: WorkMutationKindDto::Switch,
                    work_id: Some(work_id),
                    previous_work_id: Some(work_id),
                });
            }
            WorkStatus::Active => return Err(WorkMutationError::WorkNotSwitchable(work_id)),
        }
        let previous_work_id = self.active_work_id;
        if let Some(active_id) = previous_work_id {
            let active = self
                .works
                .iter_mut()
                .find(|work| work.id == active_id)
                .ok_or_else(|| {
                    WorkStateError::Invalid("active work is missing during switch".into())
                })?;
            active.status = WorkStatus::Paused;
            active.updated_at_ms = now_ms;
        }
        let target = self.works.get_mut(target_index).ok_or_else(|| {
            WorkStateError::Invalid("target work disappeared during switch".into())
        })?;
        target.status = WorkStatus::Active;
        target.updated_at_ms = now_ms;
        self.active_work_id = Some(work_id);
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Switch,
            work_id: Some(work_id),
            previous_work_id,
        })
    }

    fn finish(&mut self, now_ms: u64) -> Result<WorkMutationOutcomeDto, WorkMutationError> {
        if !self.stack.is_empty() {
            return Err(WorkMutationError::StackNotEmpty);
        }
        let active_id = self.active_work_id.ok_or(WorkMutationError::NoActiveWork)?;
        let active = self
            .works
            .iter_mut()
            .find(|work| work.id == active_id)
            .ok_or_else(|| {
                WorkStateError::Invalid("active work is missing during finish".into())
            })?;
        active.status = WorkStatus::Done;
        active.updated_at_ms = now_ms;
        active.finished_at_ms = Some(now_ms);
        self.active_work_id = None;
        Ok(WorkMutationOutcomeDto {
            kind: WorkMutationKindDto::Finish,
            work_id: Some(active_id),
            previous_work_id: None,
        })
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

    pub fn to_prompt_block(&self, budget_bytes: usize) -> Option<MemoryBlock> {
        let active_id = self.active_work_id?;
        let active = self.works.iter().find(|work| work.id == active_id)?;
        let header = "[active work]\n";
        let footer = "[/active work]";
        let footer_len = footer.len();
        if header.len() + footer_len > budget_bytes {
            return None;
        }

        let mut out = String::from(header);
        if !append_work_section(
            &mut out,
            &format!("Goal:\n  {}\n", active.goal),
            budget_bytes,
            footer_len,
        ) {
            return Some(finalize_work_block(out, footer, budget_bytes));
        }
        if let Some(focus) = active.focus.as_deref() {
            if !append_work_section(
                &mut out,
                &format!("Focus:\n  {}\n", focus),
                budget_bytes,
                footer_len,
            ) {
                return Some(finalize_work_block(out, footer, budget_bytes));
            }
        }

        let recent_decisions: Vec<_> = self
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.work_id == active_id && entry.kind == WorkEntryKind::Decision)
            .take(3)
            .collect();
        if !recent_decisions.is_empty() {
            if !append_work_section(&mut out, "Recent decisions:\n", budget_bytes, footer_len) {
                return Some(finalize_work_block(out, footer, budget_bytes));
            }
            for entry in recent_decisions {
                if !append_work_section(
                    &mut out,
                    &format!("  - {}\n", entry.text),
                    budget_bytes,
                    footer_len,
                ) {
                    return Some(finalize_work_block(out, footer, budget_bytes));
                }
            }
        }

        Some(finalize_work_block(out, footer, budget_bytes))
    }
}

fn append_work_section(
    out: &mut String,
    section: &str,
    budget_bytes: usize,
    footer_len: usize,
) -> bool {
    let available = budget_bytes.saturating_sub(out.len() + footer_len);
    if available == 0 {
        return false;
    }
    if section.len() <= available {
        out.push_str(section);
        true
    } else {
        let end = section.floor_char_boundary(available);
        out.push_str(&section[..end]);
        false
    }
}

fn finalize_work_block(mut out: String, footer: &str, budget_bytes: usize) -> MemoryBlock {
    if out.len() + footer.len() <= budget_bytes {
        out.push_str(footer);
    }
    MemoryBlock { content: out }
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

impl From<WorkEntryKindDto> for WorkEntryKind {
    fn from(value: WorkEntryKindDto) -> Self {
        match value {
            WorkEntryKindDto::Note => Self::Note,
            WorkEntryKindDto::Idea => Self::Idea,
            WorkEntryKindDto::Decision => Self::Decision,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase1_start_rejects_non_empty_stack_without_state_change() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 2,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: vec![1],
            works: vec![
                work(1, WorkStatus::Paused, None),
                work(2, WorkStatus::Active, Some(1)),
            ],
            entries: Vec::new(),
        };
        state.validate().expect("valid stacked state");
        let before = state.clone();
        let error = state
            .apply(
                &WorkOperationDto::Start {
                    goal: "must reject".into(),
                },
                10,
            )
            .expect_err("start with stack must fail");
        assert_eq!(error, WorkMutationError::StackNotEmpty);
        assert_eq!(state, before);
    }

    #[test]
    fn phase2_switch_transitions_paused_or_deferred_work_atomically() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 3,
            next_work_id: 4,
            active_work_id: Some(2),
            stack: Vec::new(),
            works: vec![
                work(1, WorkStatus::Paused, None),
                work(2, WorkStatus::Active, None),
                work(3, WorkStatus::Deferred, None),
            ],
            entries: Vec::new(),
        };
        state.validate().expect("valid switch state");
        let before = state.clone();
        let outcome = state
            .apply(&WorkOperationDto::Switch { work_id: 3 }, 10)
            .expect("switch should succeed");
        assert_eq!(outcome.kind, WorkMutationKindDto::Switch);
        assert_eq!(outcome.work_id, Some(3));
        assert_eq!(outcome.previous_work_id, Some(2));
        assert_eq!(state.active_work_id, Some(3));
        assert_eq!(state.works[1].status, WorkStatus::Paused);
        assert_eq!(state.works[2].status, WorkStatus::Active);
        assert_eq!(state.works[1].updated_at_ms, 10);
        assert_eq!(state.works[2].updated_at_ms, 10);
        assert_eq!(state.works[0].status, WorkStatus::Paused);
        assert_eq!(state.revision, before.revision);
        assert_eq!(state.stack, before.stack);
        assert_eq!(state.works[0].updated_at_ms, before.works[0].updated_at_ms);
    }

    #[test]
    fn phase2_switch_to_current_active_is_idempotent() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 4,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: Vec::new(),
            works: vec![
                work(1, WorkStatus::Paused, None),
                work(2, WorkStatus::Active, None),
            ],
            entries: Vec::new(),
        };
        state.validate().expect("valid idempotent switch state");
        let before = state.clone();
        let outcome = state
            .apply(&WorkOperationDto::Switch { work_id: 2 }, 11)
            .expect("switch to current active should succeed");
        assert_eq!(outcome.kind, WorkMutationKindDto::Switch);
        assert_eq!(outcome.work_id, Some(2));
        assert_eq!(outcome.previous_work_id, Some(2));
        assert_eq!(state, before);
    }

    #[test]
    fn phase2_finish_marks_active_done_and_unsets_active() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 4,
            next_work_id: 2,
            active_work_id: Some(1),
            stack: Vec::new(),
            works: vec![work(1, WorkStatus::Active, None)],
            entries: Vec::new(),
        };
        state.validate().expect("valid finish state");
        let before = state.clone();
        let outcome = state
            .apply(&WorkOperationDto::Finish, 11)
            .expect("finish should succeed");
        assert_eq!(outcome.kind, WorkMutationKindDto::Finish);
        assert_eq!(outcome.work_id, Some(1));
        assert_eq!(outcome.previous_work_id, None);
        assert_eq!(state.active_work_id, None);
        assert_eq!(state.works[0].status, WorkStatus::Done);
        assert_eq!(state.works[0].finished_at_ms, Some(11));
        assert_eq!(state.works[0].updated_at_ms, 11);
        assert_eq!(state.revision, before.revision);
        assert_eq!(state.stack, before.stack);
    }

    #[test]
    fn phase2_active_missing_mutations_fail_without_state_change() {
        for operation in [
            WorkOperationDto::Focus {
                text: "next".into(),
            },
            WorkOperationDto::AddEntry {
                kind: WorkEntryKindDto::Note,
                text: "note".into(),
            },
            WorkOperationDto::Push {
                goal: "child".into(),
            },
            WorkOperationDto::Pop,
            WorkOperationDto::Finish,
        ] {
            let mut state = WorkState::default();
            let before = state.clone();
            let error = state
                .apply(&operation, 12)
                .expect_err("operation should require active work");
            assert_eq!(error, WorkMutationError::NoActiveWork);
            assert_eq!(state, before);
        }
    }

    #[test]
    fn phase2_switch_rejects_missing_done_or_abandoned_targets_without_state_change() {
        let mut missing = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 7,
            next_work_id: 2,
            active_work_id: Some(1),
            stack: Vec::new(),
            works: vec![work(1, WorkStatus::Active, None)],
            entries: Vec::new(),
        };
        missing.validate().expect("valid missing target state");
        let before = missing.clone();
        let error = missing
            .apply(&WorkOperationDto::Switch { work_id: 99 }, 13)
            .expect_err("missing target should fail");
        assert_eq!(error, WorkMutationError::WorkNotFound(99));
        assert_eq!(missing, before);

        let mut done = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 8,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: Vec::new(),
            works: vec![
                work(1, WorkStatus::Done, None),
                work(2, WorkStatus::Active, None),
            ],
            entries: Vec::new(),
        };
        done.validate().expect("valid done target state");
        let before = done.clone();
        let error = done
            .apply(&WorkOperationDto::Switch { work_id: 1 }, 14)
            .expect_err("done target should fail");
        assert_eq!(error, WorkMutationError::WorkAlreadyDone(1));
        assert_eq!(done, before);

        let mut abandoned = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 9,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: Vec::new(),
            works: vec![
                work(1, WorkStatus::Abandoned, None),
                work(2, WorkStatus::Active, None),
            ],
            entries: Vec::new(),
        };
        abandoned.validate().expect("valid abandoned target state");
        let before = abandoned.clone();
        let error = abandoned
            .apply(&WorkOperationDto::Switch { work_id: 1 }, 15)
            .expect_err("abandoned target should fail");
        assert_eq!(error, WorkMutationError::WorkAbandoned(1));
        assert_eq!(abandoned, before);
    }

    #[test]
    fn phase2_root_transitions_reject_non_empty_stack_without_state_change() {
        let state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 10,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: vec![1],
            works: vec![
                work(1, WorkStatus::Paused, None),
                work(2, WorkStatus::Active, Some(1)),
            ],
            entries: Vec::new(),
        };
        state.validate().expect("valid stacked state");
        for operation in [
            WorkOperationDto::Start {
                goal: "new root".into(),
            },
            WorkOperationDto::Switch { work_id: 1 },
            WorkOperationDto::Finish,
        ] {
            let mut current = state.clone();
            let before = current.clone();
            let error = current
                .apply(&operation, 16)
                .expect_err("stacked root transition should fail");
            assert_eq!(error, WorkMutationError::StackNotEmpty);
            assert_eq!(current, before);
        }
    }

    #[test]
    fn phase3_push_creates_child_and_preserves_parent_chain() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 11,
            next_work_id: 2,
            active_work_id: Some(1),
            stack: Vec::new(),
            works: vec![work(1, WorkStatus::Active, None)],
            entries: Vec::new(),
        };
        state.validate().expect("valid push state");
        let before = state.clone();
        let outcome = state
            .apply(
                &WorkOperationDto::Push {
                    goal: "child work".into(),
                },
                20,
            )
            .expect("push should succeed");
        assert_eq!(outcome.kind, WorkMutationKindDto::Push);
        assert_eq!(outcome.work_id, Some(2));
        assert_eq!(outcome.previous_work_id, Some(1));
        assert_eq!(state.active_work_id, Some(2));
        assert_eq!(state.stack, vec![1]);
        assert_eq!(state.works[0].status, WorkStatus::Paused);
        assert_eq!(state.works[1].status, WorkStatus::Active);
        assert_eq!(state.works[1].parent_id, Some(1));
        assert_eq!(state.works[0].updated_at_ms, 20);
        assert_eq!(state.works[1].updated_at_ms, 20);
        assert_eq!(state.revision, before.revision);
    }

    #[test]
    fn phase3_nested_push_preserves_parent_chain() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 12,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: vec![1],
            works: vec![
                work(1, WorkStatus::Paused, None),
                work(2, WorkStatus::Active, Some(1)),
            ],
            entries: Vec::new(),
        };
        state.validate().expect("valid nested push state");
        let outcome = state
            .apply(
                &WorkOperationDto::Push {
                    goal: "grandchild".into(),
                },
                21,
            )
            .expect("nested push should succeed");
        assert_eq!(outcome.kind, WorkMutationKindDto::Push);
        assert_eq!(outcome.work_id, Some(3));
        assert_eq!(outcome.previous_work_id, Some(2));
        assert_eq!(state.active_work_id, Some(3));
        assert_eq!(state.stack, vec![1, 2]);
        assert_eq!(state.works[1].status, WorkStatus::Paused);
        assert_eq!(state.works[2].status, WorkStatus::Active);
        assert_eq!(state.works[2].parent_id, Some(2));
    }

    #[test]
    fn phase3_pop_finishes_child_and_restores_parent() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 13,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: vec![1],
            works: vec![
                work(1, WorkStatus::Paused, None),
                WorkItem {
                    parent_id: Some(1),
                    ..work(2, WorkStatus::Active, Some(1))
                },
            ],
            entries: vec![
                WorkEntry {
                    id: 1,
                    work_id: 2,
                    kind: WorkEntryKind::Decision,
                    text: "child decision".into(),
                    created_at_ms: 1,
                },
                WorkEntry {
                    id: 2,
                    work_id: 2,
                    kind: WorkEntryKind::Note,
                    text: "child note".into(),
                    created_at_ms: 1,
                },
            ],
        };
        state.validate().expect("valid pop state");
        let outcome = state
            .apply(&WorkOperationDto::Pop, 22)
            .expect("pop should succeed");
        assert_eq!(outcome.kind, WorkMutationKindDto::Pop);
        assert_eq!(outcome.work_id, Some(2));
        assert_eq!(outcome.previous_work_id, Some(1));
        assert_eq!(state.active_work_id, Some(1));
        assert_eq!(state.stack, Vec::<u64>::new());
        assert_eq!(state.works[0].status, WorkStatus::Active);
        assert_eq!(state.works[1].status, WorkStatus::Done);
        assert_eq!(state.works[1].finished_at_ms, Some(22));
        assert_eq!(state.works[1].updated_at_ms, 22);
        assert_eq!(state.entries.len(), 2);
    }

    #[test]
    fn phase3_pop_rejects_empty_stack_without_state_change() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 14,
            next_work_id: 2,
            active_work_id: Some(1),
            stack: Vec::new(),
            works: vec![work(1, WorkStatus::Active, None)],
            entries: Vec::new(),
        };
        state.validate().expect("valid empty stack state");
        let before = state.clone();
        let error = state
            .apply(&WorkOperationDto::Pop, 23)
            .expect_err("pop should fail when stack empty");
        assert_eq!(error, WorkMutationError::EmptyStack);
        assert_eq!(state, before);
    }

    #[test]
    fn phase3_pop_does_not_merge_child_entries_into_parent() {
        let mut state = WorkState {
            schema_version: WORK_SCHEMA_VERSION,
            revision: 15,
            next_work_id: 3,
            active_work_id: Some(2),
            stack: vec![1],
            works: vec![
                work(1, WorkStatus::Paused, None),
                WorkItem {
                    parent_id: Some(1),
                    ..work(2, WorkStatus::Active, Some(1))
                },
            ],
            entries: vec![
                WorkEntry {
                    id: 1,
                    work_id: 2,
                    kind: WorkEntryKind::Decision,
                    text: "child decision".into(),
                    created_at_ms: 1,
                },
                WorkEntry {
                    id: 2,
                    work_id: 2,
                    kind: WorkEntryKind::Idea,
                    text: "child idea".into(),
                    created_at_ms: 1,
                },
            ],
        };
        state.validate().expect("valid pop merge state");
        let before = state.clone();
        let _ = state.apply(&WorkOperationDto::Pop, 24).expect("pop");
        assert_eq!(state.entries, before.entries);
        assert_eq!(state.works[0].status, WorkStatus::Active);
        assert_eq!(state.works[1].status, WorkStatus::Done);
    }

    #[test]
    fn apply_rejects_invalid_input_without_changing_state() {
        let mut state = WorkState::default();
        let before = state.clone();
        let error = state
            .apply(&WorkOperationDto::Start { goal: " ".into() }, 10)
            .expect_err("blank goal must fail");
        assert_eq!(
            error,
            WorkMutationError::InvalidOperation(WorkInputError::EmptyText)
        );
        assert_eq!(state, before);
    }

    fn work(id: u64, status: WorkStatus, parent_id: Option<u64>) -> WorkItem {
        WorkItem {
            id,
            title: format!("work {id}"),
            goal: format!("work {id}"),
            status,
            parent_id,
            created_at_ms: id,
            updated_at_ms: id,
            finished_at_ms: None,
            focus: None,
            summary: None,
        }
    }
}
