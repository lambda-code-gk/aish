//! Work snapshot の CLI 表示。

use aibe_protocol::{WorkEntryKindDto, WorkSnapshotDto, WorkStatusDto};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkView {
    Dashboard,
    Status,
    List,
}

const DISPLAY_LIMIT_DECISIONS: usize = 5;
const DISPLAY_LIMIT_IDEAS: usize = 5;
const DISPLAY_LIMIT_NOTES: usize = 5;
const DISPLAY_LIMIT_DEFERRED: usize = 10;
const DISPLAY_LIMIT_STACK: usize = 5;

pub fn render_work_snapshot(snapshot: &WorkSnapshotDto, view: WorkView) -> String {
    match view {
        WorkView::Dashboard if snapshot.active_work_id.is_none() => {
            "No active work.\n\nStart a new work:\n  ai work start \"...\"\n\nUseful commands:\n  ai work list\n  ai work defer \"...\"".into()
        }
        WorkView::Status if snapshot.active_work_id.is_none() => {
            "No active work.\n\nStart one:\n  ai work start \"...\"".into()
        }
        WorkView::List => render_list(snapshot),
        WorkView::Dashboard | WorkView::Status => render_status(snapshot),
    }
}

fn render_status(snapshot: &WorkSnapshotDto) -> String {
    let Some(active_id) = snapshot.active_work_id else {
        return "No active work.".into();
    };
    let Some(active) = snapshot.works.iter().find(|work| work.id == active_id) else {
        return "No active work.".into();
    };
    let mut out = format!("Active work:\n  #{} {}\n", active.id, active.title);
    out.push_str("\nFocus:\n  ");
    out.push_str(active.focus.as_deref().unwrap_or("(none)"));
    out.push_str("\n\nStack:\n");
    if snapshot.stack.is_empty() {
        out.push_str("  (empty)\n");
    } else {
        let (visible, hidden) = take_recent_with_more(&snapshot.stack, DISPLAY_LIMIT_STACK);
        for id in visible.iter().rev() {
            if let Some(work) = snapshot.works.iter().find(|work| work.id == *id) {
                out.push_str(&format!("  #{} {}\n", work.id, work.title));
            }
        }
        append_more_line(&mut out, hidden);
    }
    render_entries(
        &mut out,
        snapshot,
        active_id,
        WorkEntryKindDto::Decision,
        "Decisions",
        DISPLAY_LIMIT_DECISIONS,
    );
    render_entries(
        &mut out,
        snapshot,
        active_id,
        WorkEntryKindDto::Idea,
        "Ideas",
        DISPLAY_LIMIT_IDEAS,
    );
    render_entries(
        &mut out,
        snapshot,
        active_id,
        WorkEntryKindDto::Note,
        "Notes",
        DISPLAY_LIMIT_NOTES,
    );
    out.push_str("\nDeferred:\n");
    let deferred: Vec<_> = snapshot
        .works
        .iter()
        .filter(|work| work.status == WorkStatusDto::Deferred)
        .collect();
    if deferred.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let (visible, hidden) = take_recent_with_more(&deferred, DISPLAY_LIMIT_DEFERRED);
        for work in visible {
            out.push_str(&format!("  #{} {}\n", work.id, work.title));
        }
        append_more_line(&mut out, hidden);
    }
    out.push_str(
        "\nSuggested next:\n  ai work focus <text>\n  ai work push <goal>\n  ai work defer <text>",
    );
    out
}

fn take_recent_with_more<T>(items: &[T], limit: usize) -> (&[T], usize) {
    if items.len() <= limit {
        (items, 0)
    } else {
        let hidden = items.len() - limit;
        (&items[items.len() - limit..], hidden)
    }
}

fn append_more_line(out: &mut String, hidden: usize) {
    if hidden > 0 {
        out.push_str(&format!("  ... and {hidden} more\n"));
    }
}

fn render_entries(
    out: &mut String,
    snapshot: &WorkSnapshotDto,
    active_id: u64,
    kind: WorkEntryKindDto,
    label: &str,
    limit: usize,
) {
    out.push_str(&format!("\n{label}:\n"));
    let entries: Vec<_> = snapshot
        .entries
        .iter()
        .filter(|entry| entry.work_id == active_id && entry.kind == kind)
        .collect();
    if entries.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let (visible, hidden) = take_recent_with_more(&entries, limit);
        for entry in visible {
            out.push_str(&format!("  - {}\n", entry.text));
        }
        append_more_line(out, hidden);
    }
}

fn render_list(snapshot: &WorkSnapshotDto) -> String {
    let groups = [
        ("Active", WorkStatusDto::Active),
        ("Paused", WorkStatusDto::Paused),
        ("Deferred", WorkStatusDto::Deferred),
        ("Done", WorkStatusDto::Done),
    ];
    let mut sections = Vec::new();
    for (label, status) in groups {
        let mut section = format!("{label}:\n");
        let works: Vec<_> = snapshot
            .works
            .iter()
            .filter(|work| work.status == status)
            .collect();
        if works.is_empty() {
            section.push_str("  (none)");
        } else {
            for work in works {
                let stack_marker = if snapshot.stack.contains(&work.id) {
                    " [stack]"
                } else {
                    ""
                };
                section.push_str(&format!("  #{} {}{}\n", work.id, work.title, stack_marker));
            }
            section.pop();
        }
        sections.push(section);
    }
    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::{WorkEntryDto, WorkItemDto};

    #[test]
    fn take_recent_with_more_returns_all_when_within_limit() {
        let items = [1, 2, 3];
        let (visible, hidden) = take_recent_with_more(&items, 5);
        assert_eq!(visible, &[1, 2, 3]);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn take_recent_with_more_keeps_most_recent_items() {
        let items = [1, 2, 3, 4, 5, 6, 7];
        let (visible, hidden) = take_recent_with_more(&items, 5);
        assert_eq!(visible, &[3, 4, 5, 6, 7]);
        assert_eq!(hidden, 2);
    }

    #[test]
    fn status_renders_notes_and_truncates_sections() {
        let mut entries = Vec::new();
        for index in 1..=6 {
            entries.push(WorkEntryDto {
                id: index,
                work_id: 1,
                kind: WorkEntryKindDto::Decision,
                text: format!("decision {index}"),
                created_at_ms: index,
            });
        }
        for index in 7..=9 {
            entries.push(WorkEntryDto {
                id: index,
                work_id: 1,
                kind: WorkEntryKindDto::Note,
                text: format!("note {index}"),
                created_at_ms: index,
            });
        }
        let snapshot = WorkSnapshotDto {
            revision: 1,
            active_work_id: Some(1),
            stack: Vec::new(),
            works: vec![WorkItemDto {
                id: 1,
                title: "active".into(),
                goal: "active".into(),
                status: WorkStatusDto::Active,
                parent_id: None,
                created_at_ms: 1,
                updated_at_ms: 1,
                finished_at_ms: None,
                focus: None,
                summary: None,
            }],
            entries,
        };
        let rendered = render_work_snapshot(&snapshot, WorkView::Status);
        assert!(rendered.contains("Notes:\n  - note 7"));
        assert!(rendered.contains("  - note 9"));
        assert!(!rendered.contains("note 6"));
        assert!(rendered.contains("Decisions:\n  - decision 2"));
        assert!(rendered.contains("  - decision 6"));
        assert!(!rendered.contains("decision 1"));
        assert!(rendered.contains("  ... and 1 more"));
    }

    #[test]
    fn status_truncates_stack_from_top_not_bottom() {
        let works: Vec<WorkItemDto> = (1..=6)
            .map(|id| WorkItemDto {
                id,
                title: format!("work {id}"),
                goal: format!("work {id}"),
                status: WorkStatusDto::Paused,
                parent_id: None,
                created_at_ms: id,
                updated_at_ms: id,
                finished_at_ms: None,
                focus: None,
                summary: None,
            })
            .chain(std::iter::once(WorkItemDto {
                id: 7,
                title: "active".into(),
                goal: "active".into(),
                status: WorkStatusDto::Active,
                parent_id: Some(6),
                created_at_ms: 7,
                updated_at_ms: 7,
                finished_at_ms: None,
                focus: None,
                summary: None,
            }))
            .collect();
        let snapshot = WorkSnapshotDto {
            revision: 1,
            active_work_id: Some(7),
            stack: vec![1, 2, 3, 4, 5, 6],
            works,
            entries: Vec::new(),
        };
        let rendered = render_work_snapshot(&snapshot, WorkView::Status);
        assert!(rendered.contains("Stack:\n  #6 work 6"));
        assert!(rendered.contains("  #2 work 2"));
        assert!(!rendered.contains("  #1 work 1"));
        assert!(rendered.contains("  ... and 1 more"));
    }
}
