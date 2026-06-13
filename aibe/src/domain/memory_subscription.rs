//! memory 変更通知の domain 型。

use std::collections::HashMap;

use aibe_protocol::{MemoryChangeEventDto, MemoryChangeKind, MemoryEntryDto, MemoryOperationDto};

use crate::domain::MemoryEntry;
use crate::ports::outbound::MemorySubscriptionBroker;

/// subscriber の kind filter。
#[derive(Debug, Clone, Default)]
pub struct MemorySubscriptionFilter {
    pub kind: Option<String>,
}

/// broker が配信する 1 件の変更イベント。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChangeEvent {
    pub kind: String,
    pub change: MemoryChangeKind,
    pub entries: Vec<MemoryEntryDto>,
}

impl MemoryChangeEvent {
    pub fn to_dto(&self) -> MemoryChangeEventDto {
        MemoryChangeEventDto {
            kind: self.kind.clone(),
            change: self.change,
            entries: self.entries.clone(),
        }
    }
}

pub fn change_kind_for_operation(operation: &MemoryOperationDto) -> MemoryChangeKind {
    match operation {
        MemoryOperationDto::Add(_) => MemoryChangeKind::Added,
        MemoryOperationDto::ClearKind(_) => MemoryChangeKind::StatusChanged,
        MemoryOperationDto::Archive(_) => MemoryChangeKind::Archived,
    }
}

pub fn memory_change_events_from_entries(
    change: MemoryChangeKind,
    entries: &[MemoryEntry],
) -> Vec<MemoryChangeEvent> {
    let mut grouped: HashMap<String, Vec<MemoryEntryDto>> = HashMap::new();
    for entry in entries {
        grouped
            .entry(entry.kind.clone())
            .or_default()
            .push(entry.to_dto());
    }
    grouped
        .into_iter()
        .map(|(kind, entries)| MemoryChangeEvent {
            kind,
            change,
            entries,
        })
        .collect()
}

pub fn publish_memory_changes(
    broker: &dyn MemorySubscriptionBroker,
    memory_space_id: &str,
    change: MemoryChangeKind,
    entries: &[MemoryEntry],
) {
    for event in memory_change_events_from_entries(change, entries) {
        broker.publish(memory_space_id, event);
    }
}
