//! AIBE 生成 unified diff preview（設計 §16）。

use crate::domain::{DiffPreview, DiffSummary, FileChangeOperation};

const CONTEXT_LINES: usize = 3;

/// 変更前後から unified diff preview を生成する。
pub fn build_unified_diff_preview(
    display_path: &str,
    before: Option<&[u8]>,
    after: &[u8],
    operation: FileChangeOperation,
    max_preview_bytes: usize,
) -> DiffPreview {
    let before_bytes = before.map_or(0, |b| b.len());
    let after_bytes = after.len();
    let old_lines = before.map(split_text_lines).unwrap_or_default();
    let new_lines = split_text_lines(after);

    let (lines_removed, lines_added) = count_line_changes(&old_lines, &new_lines);
    let mut diff_body = render_unified_diff(display_path, &old_lines, &new_lines, before.is_none());

    let preview_truncated = diff_body.len() > max_preview_bytes;
    if preview_truncated {
        diff_body = truncate_at_line_boundary(&diff_body, max_preview_bytes);
    }

    DiffPreview {
        diff_text: diff_body,
        summary: DiffSummary {
            operation,
            lines_added,
            lines_removed,
            before_bytes,
            after_bytes,
        },
        preview_truncated,
    }
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

        let ctx_start = change_start.saturating_sub(CONTEXT_LINES);
        let old_hunk_start = old_lines[..ctx_start].len() + 1;
        let new_hunk_start = new_lines[..ctx_start].len() + 1;

        let mut hunk_old = 0usize;
        let mut hunk_new = 0usize;
        let mut hunk_lines: Vec<(char, String)> = Vec::new();

        let mut o = ctx_start;
        let mut n = ctx_start;
        for op in &ops[ctx_start..idx] {
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

fn diff_ops(old: &[String], new: &[String]) -> Vec<DiffOp> {
    let n = old.len();
    let m = new.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            if old[i] == new[j] {
                dp[i][j] = dp[i + 1][j + 1] + 1;
            } else {
                dp[i][j] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    let mut ops = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < n && j < m {
        if old[i] == new[j] {
            ops.push(DiffOp::Equal);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(DiffOp::Delete);
            i += 1;
        } else {
            ops.push(DiffOp::Insert);
            j += 1;
        }
    }
    while i < n {
        ops.push(DiffOp::Delete);
        i += 1;
    }
    while j < m {
        ops.push(DiffOp::Insert);
        j += 1;
    }
    ops
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
}
