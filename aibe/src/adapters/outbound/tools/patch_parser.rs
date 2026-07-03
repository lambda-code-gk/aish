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
    Context(String),
    Remove(String),
    Add(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchError {
    InvalidPatch,
    PatchConflict,
    OverlappingHunks,
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
                    PatchLine::Context(s) | PatchLine::Remove(s) => {
                        *s = strip_trailing_newline(s);
                    }
                    PatchLine::Add(_) => return Err(PatchError::InvalidPatch),
                }
                continue;
            }

            let (prefix, rest) = raw
                .chars()
                .next()
                .map(|c| (c, &raw[c.len_utf8()..]))
                .ok_or(PatchError::InvalidPatch)?;
            let parsed = match prefix {
                ' ' => PatchLine::Context(rest.to_string()),
                '-' => PatchLine::Remove(rest.to_string()),
                '+' => PatchLine::Add(rest.to_string()),
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
            PatchLine::Context(_) => {
                old += 1;
                new += 1;
            }
            PatchLine::Remove(_) => old += 1,
            PatchLine::Add(_) => new += 1,
        }
    }
    (old, new)
}

fn strip_trailing_newline(line: &str) -> String {
    if let Some(stripped) = line.strip_suffix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = line.strip_suffix('\n') {
        stripped.to_string()
    } else {
        line.to_string()
    }
}

/// ファイル行へ hunk を適用する（fuzzy match なし）。
pub(crate) fn apply_hunks_to_lines(
    file_lines: &[String],
    hunks: &[PatchHunk],
) -> Result<Vec<String>, PatchError> {
    let mut lines = file_lines.to_vec();
    let mut line_offset = 0i64;

    for hunk in hunks {
        let pos = (hunk.old_start as i64 - 1 + line_offset) as usize;
        if pos > lines.len() {
            return Err(PatchError::PatchConflict);
        }

        let mut verify_idx = pos;
        for hunk_line in &hunk.lines {
            match hunk_line {
                PatchLine::Context(expected) | PatchLine::Remove(expected) => {
                    if verify_idx >= lines.len() || &lines[verify_idx] != expected {
                        return Err(PatchError::PatchConflict);
                    }
                    verify_idx += 1;
                }
                PatchLine::Add(_) => {}
            }
        }

        let remove_end = verify_idx;
        let mut replacement = Vec::new();
        for hunk_line in &hunk.lines {
            match hunk_line {
                PatchLine::Context(s) | PatchLine::Add(s) => replacement.push(s.clone()),
                PatchLine::Remove(_) => {}
            }
        }
        lines.splice(pos..remove_end, replacement);
        line_offset += hunk.new_len as i64 - hunk.old_len as i64;
    }

    Ok(lines)
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

    fn lines(text: &str) -> Vec<String> {
        split_file_lines(text.as_bytes(), LineEnding::Lf).lines
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
    fn applies_single_hunk() {
        let file = lines("line1\nline2\nline3\n");
        let patch = "@@ -2,1 +2,1 @@\n-line2\n+LINE2\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        let result = apply_hunks_to_lines(&file, &hunks).expect("apply");
        assert_eq!(result, lines("line1\nLINE2\nline3\n"));
    }

    #[test]
    fn context_mismatch_is_conflict() {
        let file = lines("line1\nline2\n");
        let patch = "@@ -2,1 +2,1 @@\n-wrong\n+new\n";
        let hunks = parse_unified_hunks(patch).expect("parse");
        assert_eq!(
            apply_hunks_to_lines(&file, &hunks),
            Err(PatchError::PatchConflict)
        );
    }
}
