//! strict unified hunk parser（設計 §10.3–10.5）。

use crate::domain::LineEnding;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PatchHunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub lines: Vec<PatchLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatchLine {
    Context(String, bool),
    Remove(String, bool),
    Add(String, bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchError {
    InvalidPatch,
    PatchConflict,
    OverlappingHunks,
}

#[derive(Debug, Clone)]
pub(crate) struct AppliedPatch {
    pub lines: Vec<String>,
    pub trailing_newline: bool,
}

pub(crate) fn parse_unified_hunks(patch: &str) -> Result<Vec<PatchHunk>, PatchError> {
    let trimmed = patch.trim();
    if trimmed.is_empty() {
        return Err(PatchError::InvalidPatch);
    }

    if trimmed.starts_with("--- ") || trimmed.starts_with("+++ ") || trimmed.starts_with("diff ") {
        return Err(PatchError::InvalidPatch);
    }

    let mut hunks = Vec::new();
    let mut lines = patch.split('\n').peekable();

    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            return Err(PatchError::InvalidPatch);
        }
        if !line.starts_with("@@ ") {
            return Err(PatchError::InvalidPatch);
        }
        if !line.ends_with(" @@") {
            return Err(PatchError::InvalidPatch);
        }

        let header = &line[3..line.len() - 3];
        let (old_start, old_len, new_start, new_len) = parse_hunk_header(header)?;

        let mut hunk_lines = Vec::new();
        while let Some(&next) = lines.peek() {
            if next.is_empty() {
                lines.next();
                continue;
            }
            if next.starts_with("@@ ") {
                break;
            }
            let raw = lines.next().expect("peeked");
            if raw == "\\ No newline at end of file" {
                let Some(last) = hunk_lines.last_mut() else {
                    return Err(PatchError::InvalidPatch);
                };
                match last {
                    PatchLine::Context(_, no_newline)
                    | PatchLine::Remove(_, no_newline)
                    | PatchLine::Add(_, no_newline) => {
                        *no_newline = true;
                    }
                }
                continue;
            }

            let (prefix, rest) = raw
                .chars()
                .next()
                .map(|c| (c, &raw[c.len_utf8()..]))
                .ok_or(PatchError::InvalidPatch)?;
            let parsed = match prefix {
                ' ' => PatchLine::Context(rest.to_string(), false),
                '-' => PatchLine::Remove(rest.to_string(), false),
                '+' => PatchLine::Add(rest.to_string(), false),
                _ => return Err(PatchError::InvalidPatch),
            };
            hunk_lines.push(parsed);
        }

        let (actual_old, actual_new) = count_hunk_lines(&hunk_lines);
        if actual_old != old_len || actual_new != new_len {
            return Err(PatchError::InvalidPatch);
        }

        hunks.push(PatchHunk {
            old_start,
            old_len,
            new_start,
            new_len,
            lines: hunk_lines,
        });
    }

    if hunks.is_empty() {
        return Err(PatchError::InvalidPatch);
    }

    for i in 0..hunks.len().saturating_sub(1) {
        if hunks[i].old_start > hunks[i + 1].old_start {
            return Err(PatchError::InvalidPatch);
        }
        let end = hunks[i].old_start + hunks[i].old_len;
        if hunks[i + 1].old_start < end {
            return Err(PatchError::OverlappingHunks);
        }
    }

    Ok(hunks)
}

fn parse_hunk_header(header: &str) -> Result<(usize, usize, usize, usize), PatchError> {
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(PatchError::InvalidPatch);
    }
    let old = parse_range(parts[0], '-').ok_or(PatchError::InvalidPatch)?;
    let new = parse_range(parts[1], '+').ok_or(PatchError::InvalidPatch)?;
    Ok((old.0, old.1, new.0, new.1))
}

fn parse_range(part: &str, sign: char) -> Option<(usize, usize)> {
    if !part.starts_with(sign) {
        return None;
    }
    let body = &part[1..];
    if let Some((start, len)) = body.split_once(',') {
        let start: usize = start.parse().ok()?;
        let len: usize = len.parse().ok()?;
        Some((start, len))
    } else {
        let start: usize = body.parse().ok()?;
        Some((start, 1))
    }
}

fn count_hunk_lines(lines: &[PatchLine]) -> (usize, usize) {
    let mut old = 0usize;
    let mut new = 0usize;
    for line in lines {
        match line {
            PatchLine::Context(_, _) => {
                old += 1;
                new += 1;
            }
            PatchLine::Remove(_, _) => old += 1,
            PatchLine::Add(_, _) => new += 1,
        }
    }
    (old, new)
}

/// ファイル行へ hunk を適用する（fuzzy match なし）。
pub(crate) fn apply_hunks_to_lines(
    file_lines: &FileLines,
    hunks: &[PatchHunk],
) -> Result<AppliedPatch, PatchError> {
    let mut lines = file_lines.lines.clone();
    let mut trailing_newline = file_lines.trailing_newline;
    let mut line_offset = 0i64;

    for hunk in hunks {
        let expected_new_start = if hunk.old_start == 0 && hunk.old_len == 0 {
            hunk.new_start
        } else {
            let idx = hunk.old_start as i64 + line_offset;
            if idx < 1 {
                return Err(PatchError::InvalidPatch);
            }
            idx as usize
        };
        if hunk.new_start != expected_new_start {
            return Err(PatchError::InvalidPatch);
        }

        let pos = if hunk.old_start == 0 && hunk.old_len == 0 {
            0
        } else {
            let idx = hunk.old_start as i64 - 1 + line_offset;
            if idx < 0 {
                return Err(PatchError::PatchConflict);
            }
            idx as usize
        };
        if pos > lines.len() {
            return Err(PatchError::PatchConflict);
        }

        let mut verify_idx = pos;
        for hunk_line in &hunk.lines {
            match hunk_line {
                PatchLine::Context(expected, _) | PatchLine::Remove(expected, _) => {
                    if verify_idx >= lines.len() || &lines[verify_idx] != expected {
                        return Err(PatchError::PatchConflict);
                    }
                    verify_idx += 1;
                }
                PatchLine::Add(_, _) => {}
            }
        }

        let remove_end = verify_idx;
        let mut replacement = Vec::new();
        let mut replacement_last_no_newline = false;
        for hunk_line in &hunk.lines {
            match hunk_line {
                PatchLine::Context(s, no_newline) | PatchLine::Add(s, no_newline) => {
                    replacement.push(s.clone());
                    replacement_last_no_newline = *no_newline;
                }
                PatchLine::Remove(_, _) => {}
            }
        }
        lines.splice(pos..remove_end, replacement);
        if pos + hunk.new_len > lines.len() {
            return Err(PatchError::PatchConflict);
        }
        if hunk.new_len > 0 && pos + hunk.new_len == lines.len() {
            trailing_newline = !replacement_last_no_newline;
        }
        line_offset += hunk.new_len as i64 - hunk.old_len as i64;
    }

    Ok(AppliedPatch {
        lines,
        trailing_newline,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct FileLines {
    pub lines: Vec<String>,
    pub trailing_newline: bool,
}

pub(crate) fn split_file_lines(bytes: &[u8], line_ending: LineEnding) -> FileLines {
    let text = std::str::from_utf8(bytes).unwrap_or("");
    if text.is_empty() {
        return FileLines {
            lines: Vec::new(),
            trailing_newline: false,
        };
    }
    let trailing_newline = text.ends_with('\n');
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find('\n') {
        let segment = &rest[..pos];
        let logical = segment.strip_suffix('\r').unwrap_or(segment);
        lines.push(logical.to_string());
        rest = &rest[pos + 1..];
    }
    if !rest.is_empty() {
        let logical = rest.strip_suffix('\r').unwrap_or(rest);
        lines.push(logical.to_string());
    }
    let _ = line_ending;
    FileLines {
        lines,
        trailing_newline,
    }
}

pub(crate) fn encode_file_lines(
    lines: &[String],
    line_ending: LineEnding,
    trailing_newline: bool,
) -> Vec<u8> {
    if lines.is_empty() {
        return Vec::new();
    }
    let sep = match line_ending {
        LineEnding::Crlf => "\r\n",
        _ => "\n",
    };
    let mut out = lines.join(sep);
    if trailing_newline {
        out.push_str(sep);
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> FileLines {
        split_file_lines(text.as_bytes(), LineEnding::Lf)
    }

    #[test]
    fn parses_single_hunk() {
        let patch = "@@ -1,3 +1,3 @@\n line1\n-line2\n+line2b\n line3\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_len, 3);
        assert_eq!(hunks[0].new_len, 3);
    }

    #[test]
    fn rejects_diff_headers() {
        let patch = "--- a/file\n+++ b/file\n@@ -1,1 +1,1 @@\n x\n";
        assert_eq!(parse_unified_hunks(patch), Err(PatchError::InvalidPatch));
    }

    #[test]
    fn rejects_overlapping_hunks() {
        let patch = "@@ -1,2 +1,2 @@\n a\n b\n@@ -2,2 +2,2 @@\n b\n c\n";
        assert_eq!(
            parse_unified_hunks(patch),
            Err(PatchError::OverlappingHunks)
        );
    }

    #[test]
    fn rejects_invalid_new_start() {
        let file = lines("line1\nline2\n");
        let patch = "@@ -1,1 +999,1 @@\n-old\n+new\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        assert!(matches!(
            apply_hunks_to_lines(&file, &hunks),
            Err(PatchError::InvalidPatch)
        ));
    }

    #[test]
    fn applies_single_hunk() {
        let file = lines("line1\nline2\nline3\n");
        let patch = "@@ -2,1 +2,1 @@\n-line2\n+LINE2\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        let result = apply_hunks_to_lines(&file, &hunks).expect("apply");
        assert_eq!(result.lines, lines("line1\nLINE2\nline3\n").lines);
        assert!(result.trailing_newline);
    }

    #[test]
    fn context_mismatch_is_conflict() {
        let file = lines("line1\nline2\n");
        let patch = "@@ -2,1 +2,1 @@\n-wrong\n+new\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        assert!(matches!(
            apply_hunks_to_lines(&file, &hunks),
            Err(PatchError::PatchConflict)
        ));
    }

    #[test]
    fn patch_can_insert_into_empty_file() {
        let file = lines("");
        let patch = "@@ -0,0 +1,1 @@\n+first line\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        let result = apply_hunks_to_lines(&file, &hunks).expect("apply");
        assert_eq!(result.lines, vec!["first line".to_string()]);
        assert!(result.trailing_newline);
    }

    #[test]
    fn patch_can_add_trailing_newline() {
        let file = lines("line");
        let patch = "@@ -1,1 +1,1 @@\n-line\n\\ No newline at end of file\n+line\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        let result = apply_hunks_to_lines(&file, &hunks).expect("apply");
        assert_eq!(result.lines, vec!["line".to_string()]);
        assert!(result.trailing_newline);
    }

    #[test]
    fn patch_can_remove_trailing_newline() {
        let file = lines("line\n");
        let patch = "@@ -1,1 +1,1 @@\n-line\n+line\n\\ No newline at end of file\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        let result = apply_hunks_to_lines(&file, &hunks).expect("apply");
        assert_eq!(result.lines, vec!["line".to_string()]);
        assert!(!result.trailing_newline);
    }
}
