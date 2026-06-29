//! Work snapshot の CLI 表示。

use aibe_protocol::{WorkEntryKindDto, WorkSnapshotDto, WorkStatusDto};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkView {
    Dashboard,
    Status,
    List,
}

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
        for id in snapshot.stack.iter().rev() {
            if let Some(work) = snapshot.works.iter().find(|work| work.id == *id) {
                out.push_str(&format!("  #{} {}\n", work.id, work.title));
            }
        }
    }
    render_entries(
        &mut out,
        snapshot,
        active_id,
        WorkEntryKindDto::Decision,
        "Decisions",
    );
    render_entries(
        &mut out,
        snapshot,
        active_id,
        WorkEntryKindDto::Idea,
        "Ideas",
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
        for work in deferred {
            out.push_str(&format!("  #{} {}\n", work.id, work.title));
        }
    }
    out.push_str(
        "\nSuggested next:\n  ai work focus <text>\n  ai work push <goal>\n  ai work defer <text>",
    );
    out
}

fn render_entries(
    out: &mut String,
    snapshot: &WorkSnapshotDto,
    active_id: u64,
    kind: WorkEntryKindDto,
    label: &str,
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
        for entry in entries {
            out.push_str(&format!("  - {}\n", entry.text));
        }
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
