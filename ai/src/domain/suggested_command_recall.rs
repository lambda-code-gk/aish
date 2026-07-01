//! assistant 提案 shell コマンドの抽出と正規化。

use serde::{Deserialize, Serialize};

pub const SUGGESTED_COMMAND_MAX_BYTES: usize = 8 * 1024;

const SHELL_LANGUAGE_TAGS: &[&str] = &["bash", "sh", "zsh", "shell"];

/// 抽出された shell コマンド候補。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedCommandCandidate {
    pub text: String,
    pub language: String,
    pub bytes: usize,
}

/// 1 turn 分の候補 queue。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedCommandQueue {
    pub turn_id: String,
    pub captured_at: String,
    pub candidates: Vec<SuggestedCommandCandidate>,
}

/// session-scoped recall cache の正本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedCommandCache {
    pub schema_version: u32,
    pub ai_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub shell: String,
    pub updated_at: String,
    pub active_queue_index: usize,
    pub active_candidate_index: usize,
    #[serde(default)]
    pub recall_navigated: bool,
    pub queues: Vec<SuggestedCommandQueue>,
}

impl SuggestedCommandCache {
    pub fn new(
        ai_session_id: impl Into<String>,
        shell: impl Into<String>,
        updated_at: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            ai_session_id: ai_session_id.into(),
            conversation_id: None,
            shell: shell.into(),
            updated_at: updated_at.into(),
            active_queue_index: 0,
            active_candidate_index: 0,
            recall_navigated: false,
            queues: Vec::new(),
        }
    }

    pub fn append_queue(&mut self, queue: SuggestedCommandQueue) {
        self.queues.push(queue);
        self.active_queue_index = self.queues.len().saturating_sub(1);
        self.active_candidate_index = 0;
        self.recall_navigated = false;
    }

    /// 直近 turn の候補を返し、次の `Alt+.` 用にカーソルを進める（末尾で先頭へラップ）。
    pub fn next_candidate(&mut self) -> Option<String> {
        let queue = self.queues.last()?;
        let n = queue.candidates.len();
        if n == 0 {
            return None;
        }
        self.active_queue_index = self.queues.len() - 1;
        let i = self.active_candidate_index;
        self.active_candidate_index = (i + 1) % n;
        self.recall_navigated = true;
        Some(queue.candidates[i].text.clone())
    }

    /// 直近 turn の候補を返し、次の `Alt+,` 用にカーソルを戻す（先頭で末尾へラップ）。
    pub fn prev_candidate(&mut self) -> Option<String> {
        let queue = self.queues.last()?;
        let n = queue.candidates.len();
        if n == 0 {
            return None;
        }
        self.active_queue_index = self.queues.len() - 1;
        let prev_shown = if !self.recall_navigated {
            n - 1
        } else {
            let last_shown = (self.active_candidate_index + n - 1) % n;
            (last_shown + n - 1) % n
        };
        self.active_candidate_index = (prev_shown + 1) % n;
        self.recall_navigated = true;
        Some(queue.candidates[prev_shown].text.clone())
    }
}

/// fenced code block から shell 候補を抽出する。
pub fn extract_shell_candidates_from_content(
    content: &str,
    max_items: usize,
) -> Vec<SuggestedCommandCandidate> {
    let mut out = Vec::new();
    for block in iter_fenced_code_blocks(content) {
        if out.len() >= max_items {
            break;
        }
        let Some(language) = block.language else {
            continue;
        };
        if !is_shell_language_tag(&language) {
            continue;
        }
        if let Some(candidate) = normalize_shell_candidate(&block.body, &language) {
            out.push(candidate);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FencedBlock {
    language: Option<String>,
    body: String,
}

fn iter_fenced_code_blocks(content: &str) -> Vec<FencedBlock> {
    let mut blocks = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("```") {
            continue;
        }
        let tag = trimmed.trim_start_matches('`').trim();
        let language = if tag.is_empty() {
            None
        } else {
            Some(tag.to_ascii_lowercase())
        };
        let mut body_lines = Vec::new();
        for inner in lines.by_ref() {
            if inner.trim_start().starts_with("```") {
                break;
            }
            body_lines.push(inner);
        }
        blocks.push(FencedBlock {
            language,
            body: body_lines.join("\n"),
        });
    }
    blocks
}

fn is_shell_language_tag(language: &str) -> bool {
    SHELL_LANGUAGE_TAGS.contains(&language)
}

fn normalize_shell_candidate(body: &str, language: &str) -> Option<SuggestedCommandCandidate> {
    let stripped = strip_ansi_escapes(body);
    if stripped.contains('\0') {
        return None;
    }
    if stripped
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return None;
    }
    let mut text = stripped;
    text = trim_outer_blank_lines(text);
    text = strip_uniform_prompt_prefix(text);
    text = text.trim_end_matches('\n').to_string();
    if text.is_empty() {
        return None;
    }
    if text.len() > SUGGESTED_COMMAND_MAX_BYTES {
        return None;
    }
    Some(SuggestedCommandCandidate {
        bytes: text.len(),
        text,
        language: language.to_string(),
    })
}

fn trim_outer_blank_lines(mut text: String) -> String {
    while text.starts_with('\n') {
        text.remove(0);
    }
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

fn strip_uniform_prompt_prefix(text: String) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return text;
    }
    let prefixes = ["$ ", "# ", "> "];
    let mut matched: Option<&str> = None;
    for prefix in prefixes {
        if lines
            .iter()
            .all(|line| line.is_empty() || line.starts_with(prefix))
        {
            matched = Some(prefix);
            break;
        }
    }
    let Some(prefix) = matched else {
        return text;
    };
    lines
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                line.strip_prefix(prefix).unwrap_or(line).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_ansi_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_shell_candidates_from_fenced_code_blocks() {
        let content = r#"以下のコマンドを実行してください：

```bash
git commit -m "refactor(work): apply clippy fixes"
```

補足です。
"#;
        let candidates = extract_shell_candidates_from_content(content, 8);
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].text,
            r#"git commit -m "refactor(work): apply clippy fixes""#
        );
        assert_eq!(candidates[0].language, "bash");
    }

    #[test]
    fn preserve_suggested_command_queue_order_across_multiple_fences() {
        let content = r#"```bash
git status
```

```sh
git add -A
```

```zsh
git commit -m "wip"
```
"#;
        let candidates = extract_shell_candidates_from_content(content, 8);
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].text, "git status");
        assert_eq!(candidates[1].text, "git add -A");
        assert_eq!(candidates[2].text, r#"git commit -m "wip""#);
    }

    #[test]
    fn reject_control_char_nul_and_oversized_suggested_commands() {
        let nul = "```bash\necho hi\0\n```";
        assert!(extract_shell_candidates_from_content(nul, 8).is_empty());

        let control = "```bash\necho hi\u{7}\n```";
        assert!(extract_shell_candidates_from_content(&control, 8).is_empty());

        let huge_body = "x".repeat(SUGGESTED_COMMAND_MAX_BYTES + 1);
        let huge = format!("```bash\n{huge_body}\n```");
        assert!(extract_shell_candidates_from_content(&huge, 8).is_empty());

        let unlabeled = "```\ngit status\n```";
        assert!(extract_shell_candidates_from_content(unlabeled, 8).is_empty());
    }

    #[test]
    fn strips_uniform_prompt_prefix() {
        let content = "```bash\n$ git status\n$ git diff\n```";
        let candidates = extract_shell_candidates_from_content(content, 8);
        assert_eq!(candidates[0].text, "git status\ngit diff");
    }

    #[test]
    fn cache_next_candidate_wraps_within_latest_queue() {
        let mut cache = SuggestedCommandCache::new("sess", "bash", "now");
        cache.append_queue(SuggestedCommandQueue {
            turn_id: "t1".into(),
            captured_at: "now".into(),
            candidates: vec![
                SuggestedCommandCandidate {
                    text: "git status".into(),
                    language: "bash".into(),
                    bytes: 10,
                },
                SuggestedCommandCandidate {
                    text: "git add -A".into(),
                    language: "bash".into(),
                    bytes: 8,
                },
            ],
        });
        assert_eq!(cache.next_candidate().as_deref(), Some("git status"));
        assert_eq!(cache.next_candidate().as_deref(), Some("git add -A"));
        assert_eq!(cache.next_candidate().as_deref(), Some("git status"));
    }

    #[test]
    fn cache_prev_candidate_wraps_within_latest_queue() {
        let mut cache = SuggestedCommandCache::new("sess", "bash", "now");
        cache.append_queue(SuggestedCommandQueue {
            turn_id: "t1".into(),
            captured_at: "now".into(),
            candidates: vec![
                SuggestedCommandCandidate {
                    text: "git status".into(),
                    language: "bash".into(),
                    bytes: 10,
                },
                SuggestedCommandCandidate {
                    text: "git add -A".into(),
                    language: "bash".into(),
                    bytes: 8,
                },
                SuggestedCommandCandidate {
                    text: "git commit".into(),
                    language: "bash".into(),
                    bytes: 10,
                },
            ],
        });
        assert_eq!(cache.prev_candidate().as_deref(), Some("git commit"));
        assert_eq!(cache.next_candidate().as_deref(), Some("git status"));
        assert_eq!(cache.next_candidate().as_deref(), Some("git add -A"));
        assert_eq!(cache.prev_candidate().as_deref(), Some("git status"));
    }

    #[test]
    fn cache_navigation_uses_latest_turn_only() {
        let mut cache = SuggestedCommandCache::new("sess", "bash", "now");
        cache.append_queue(SuggestedCommandQueue {
            turn_id: "old".into(),
            captured_at: "old".into(),
            candidates: vec![SuggestedCommandCandidate {
                text: "old cmd".into(),
                language: "bash".into(),
                bytes: 7,
            }],
        });
        cache.append_queue(SuggestedCommandQueue {
            turn_id: "new".into(),
            captured_at: "new".into(),
            candidates: vec![SuggestedCommandCandidate {
                text: "new cmd".into(),
                language: "bash".into(),
                bytes: 7,
            }],
        });
        assert_eq!(cache.next_candidate().as_deref(), Some("new cmd"));
        assert_eq!(cache.next_candidate().as_deref(), Some("new cmd"));
        assert_eq!(cache.prev_candidate().as_deref(), Some("new cmd"));
    }
}
