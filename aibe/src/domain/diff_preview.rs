//! AIBE 生成 unified diff preview（設計 §16）。

use similar::{ChangeTag, TextDiff};

use crate::domain::{DiffPreview, DiffSummary, FileChangeOperation};
use crate::ports::outbound::config::{DEFAULT_MAX_DIFF_LINES, DEFAULT_MAX_DIFF_WORK};

const CONTEXT_LINES: usize = 3;
const OMITTED_MESSAGE: &str = "diff preview omitted: change too large\n";

/// 変更前後から unified diff preview を生成する。
pub fn build_unified_diff_preview(
    display_path: &str,
    before: Option<&[u8]>,
    after: &[u8],
    operation: FileChangeOperation,
    max_preview_bytes: usize,
) -> DiffPreview {
    build_unified_diff_preview_bounded(
        display_path,
        before,
        after,
        operation,
        max_preview_bytes,
        DEFAULT_MAX_DIFF_LINES,
        DEFAULT_MAX_DIFF_WORK,
    )
}

/// 行数・作業量上限付きで unified diff preview を生成する。
pub fn build_unified_diff_preview_bounded(
    display_path: &str,
    before: Option<&[u8]>,
    after: &[u8],
    operation: FileChangeOperation,
    max_preview_bytes: usize,
    max_diff_lines: usize,
    max_diff_work: usize,
) -> DiffPreview {
    let before_bytes = before.map_or(0, |b| b.len());
    let after_bytes = after.len();
    let old_lines = before.map(split_text_lines).unwrap_or_default();
    let new_lines = split_text_lines(after);

    let summary = DiffSummary {
        operation,
        lines_added: 0,
        lines_removed: 0,
        before_bytes,
        after_bytes,
        line_stats_known: false,
    };

    if diff_work_exceeds_limits(
        old_lines.len(),
        new_lines.len(),
        max_diff_lines,
        max_diff_work,
    ) {
        return DiffPreview {
            diff_text: OMITTED_MESSAGE.to_string(),
            summary,
            preview_truncated: true,
        };
    }

    let (lines_removed, lines_added) = count_line_changes(&old_lines, &new_lines);
    let mut diff_body = render_unified_diff(display_path, &old_lines, &new_lines, before.is_none());

    let preview_truncated = diff_body.len() > max_preview_bytes;
    if preview_truncated {
        diff_body = truncate_at_line_boundary(&diff_body, max_preview_bytes);
    }

    DiffPreview {
        diff_text: diff_body,
        summary: DiffSummary {
            lines_added,
            lines_removed,
            line_stats_known: true,
            ..summary
        },
        preview_truncated,
    }
}

fn diff_work_exceeds_limits(
    old_line_count: usize,
    new_line_count: usize,
    max_diff_lines: usize,
    max_diff_work: usize,
) -> bool {
    if old_line_count > max_diff_lines || new_line_count > max_diff_lines {
        return true;
    }
    old_line_count.saturating_mul(new_line_count) > max_diff_work
}

fn split_text_lines(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    if text.is_empty() {
        return Vec::new();
    }
    text.split_inclusive('\n')
        .map(|line| line.to_string())
        .collect()
}

fn count_line_changes(old: &[String], new: &[String]) -> (usize, usize) {
    let mut removed = 0usize;
    let mut added = 0usize;
    for op in diff_ops(old, new) {
        match op {
            DiffOp::Delete => removed += 1,
            DiffOp::Insert => added += 1,
            DiffOp::Equal => {}
        }
    }
    (removed, added)
}

fn render_unified_diff(
    display_path: &str,
    old_lines: &[String],
    new_lines: &[String],
    is_new_file: bool,
) -> String {
    let mut out = String::new();
    if is_new_file {
        out.push_str("--- /dev/null\n");
        out.push_str(&format!("+++ b/{display_path}\n"));
    } else {
        out.push_str(&format!("--- a/{display_path}\n"));
        out.push_str(&format!("+++ b/{display_path}\n"));
    }

    let ops = diff_ops(old_lines, new_lines);
    if ops.iter().all(|op| matches!(op, DiffOp::Equal)) {
        return out;
    }

    let mut idx = 0usize;

    while idx < ops.len() {
        while idx < ops.len() && matches!(ops[idx], DiffOp::Equal) {
            idx += 1;
        }
        if idx >= ops.len() {
            break;
        }

        let change_start = idx;
        while idx < ops.len() && !matches!(ops[idx], DiffOp::Equal) {
            idx += 1;
        }

        let ctx_op_start = change_start.saturating_sub(CONTEXT_LINES);
        let (mut o, mut n) = line_positions_at_op(&ops, ctx_op_start);
        let old_hunk_start = o + 1;
        let new_hunk_start = n + 1;

        let mut hunk_old = 0usize;
        let mut hunk_new = 0usize;
        let mut hunk_lines: Vec<(char, String)> = Vec::new();

        for op in &ops[ctx_op_start..idx] {
            match op {
                DiffOp::Equal => {
                    hunk_lines.push((' ', old_lines[o].clone()));
                    hunk_old += 1;
                    hunk_new += 1;
                    o += 1;
                    n += 1;
                }
                DiffOp::Delete => {
                    hunk_lines.push(('-', old_lines[o].clone()));
                    hunk_old += 1;
                    o += 1;
                }
                DiffOp::Insert => {
                    hunk_lines.push(('+', new_lines[n].clone()));
                    hunk_new += 1;
                    n += 1;
                }
            }
        }

        out.push_str(&format!(
            "@@ -{old_hunk_start},{hunk_old} +{new_hunk_start},{hunk_new} @@\n"
        ));
        for (prefix, line) in hunk_lines {
            out.push(prefix);
            out.push_str(&line);
            if !line.ends_with('\n') {
                out.push_str("\\ No newline at end of file\n");
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffOp {
    Equal,
    Insert,
    Delete,
}

fn line_positions_at_op(ops: &[DiffOp], op_idx: usize) -> (usize, usize) {
    let mut o = 0usize;
    let mut n = 0usize;
    for (i, op) in ops.iter().enumerate() {
        if i >= op_idx {
            break;
        }
        match op {
            DiffOp::Equal => {
                o += 1;
                n += 1;
            }
            DiffOp::Delete => o += 1,
            DiffOp::Insert => n += 1,
        }
    }
    (o, n)
}

fn diff_ops(old: &[String], new: &[String]) -> Vec<DiffOp> {
    let old_refs: Vec<&str> = old.iter().map(String::as_str).collect();
    let new_refs: Vec<&str> = new.iter().map(String::as_str).collect();
    let diff = TextDiff::from_slices(&old_refs, &new_refs);
    diff.iter_all_changes()
        .map(|change| match change.tag() {
            ChangeTag::Equal => DiffOp::Equal,
            ChangeTag::Delete => DiffOp::Delete,
            ChangeTag::Insert => DiffOp::Insert,
        })
        .collect()
}

fn truncate_at_line_boundary(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    while end > 0 && !text[..end].ends_with('\n') {
        end -= 1;
    }
    if end == 0 {
        text.chars().take(max_bytes).collect()
    } else {
        text[..end].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_file_uses_dev_null_header() {
        let preview = build_unified_diff_preview(
            "src/new.rs",
            None,
            b"fn main() {}\n",
            FileChangeOperation::Create,
            32_768,
        );
        assert!(preview
            .diff_text
            .starts_with("--- /dev/null\n+++ b/src/new.rs\n"));
    }

    #[test]
    fn existing_file_uses_a_b_headers() {
        let preview = build_unified_diff_preview(
            "src/main.rs",
            Some(b"old\n"),
            b"new\n",
            FileChangeOperation::Replace,
            32_768,
        );
        assert!(preview
            .diff_text
            .starts_with("--- a/src/main.rs\n+++ b/src/main.rs\n"));
    }

    #[test]
    fn diff_preview_large_line_count_is_bounded() {
        let before: String = (0..12_000).map(|i| format!("old-{i}\n")).collect();
        let after: String = (0..12_000).map(|i| format!("new-{i}\n")).collect();
        let preview = build_unified_diff_preview_bounded(
            "big.txt",
            Some(before.as_bytes()),
            after.as_bytes(),
            FileChangeOperation::Replace,
            32_768,
            10_000,
            1_000_000,
        );
        assert_eq!(preview.diff_text, OMITTED_MESSAGE);
        assert!(preview.preview_truncated);
        assert_eq!(preview.summary.before_bytes, before.len());
        assert_eq!(preview.summary.after_bytes, after.len());
        assert!(!preview.summary.line_stats_known);
        assert!(preview
            .summary
            .display_line("big.txt")
            .contains("line changes unknown"));
    }
}
