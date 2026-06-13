//! contextual memory の resolver policy（クエリごとの注入候補選択）。

use std::collections::{HashMap, HashSet};

use super::contextual_memory::{
    format_memory_block_with_budget, MemoryBlock, MemoryEntry, MemoryInjectPolicy, MemoryScope,
    MemoryStatus,
};
use super::memory_kind_registry::{MemoryCardinality, MemoryKindDefinition, MemoryKindRegistry};

/// resolver への入力。
pub struct MemoryResolveInput<'a> {
    pub entries: &'a [MemoryEntry],
    pub registry: &'a MemoryKindRegistry,
    pub project_key: Option<&'a str>,
    pub current_session_id: &'a str,
    pub user_query: &'a str,
    pub budget_bytes: usize,
}

/// §7 の選択順に従い prompt block 用 entries を解決する。
pub struct MemoryResolverPolicy;

impl MemoryResolverPolicy {
    pub fn resolve(input: &MemoryResolveInput<'_>) -> MemoryBlock {
        let mut selected: Vec<MemoryEntry> = Vec::new();
        let mut seen_ids: HashSet<String> = HashSet::new();

        // 1. pinned auto-inject
        for def in input.registry.pinned_auto_inject_definitions() {
            let scope_key = project_scope_key(def, input.project_key);
            push_unique(
                &mut selected,
                &mut seen_ids,
                collect_kind_entries(
                    input,
                    def,
                    scope_key,
                    &[MemoryStatus::Active],
                    MemoryInjectPolicy::Pinned,
                ),
            );
        }

        // 2. explicitly requested kinds
        let explicit_kinds = collect_explicit_kinds(input.registry, input.user_query);
        for kind in &explicit_kinds {
            let Some(def) = input.registry.get(kind) else {
                continue;
            };
            if def.default_inject == MemoryInjectPolicy::Manual
                || def.default_inject == MemoryInjectPolicy::Never
            {
                continue;
            }
            let scope_key = project_scope_key(def, input.project_key);
            let statuses = explicit_statuses(def);
            push_unique(
                &mut selected,
                &mut seen_ids,
                collect_kind_entries(input, def, scope_key, &statuses, def.default_inject),
            );
        }

        // 3. related memory（Open は explicit kind のみ）
        for entry in input.entries {
            if seen_ids.contains(&entry.id) {
                continue;
            }
            if !entry_allowed_for_prompt(entry, input.project_key) {
                continue;
            }
            if entry.status == MemoryStatus::Open && !explicit_kinds.contains(&entry.kind) {
                continue;
            }
            if entry.inject == MemoryInjectPolicy::Manual
                || entry.inject == MemoryInjectPolicy::Never
            {
                continue;
            }
            if entry_related_to_query(input.registry, input.user_query, entry) {
                seen_ids.insert(entry.id.clone());
                selected.push(entry.clone());
            }
        }

        // 4. recent open — step 2 と統合済み（explicit on_demand で Open を収集）
        // 5. fallback summary — v1 未実装

        enforce_kind_limits(input.registry, &mut selected);
        sort_selected_by_priority(input.registry, &mut selected);

        let block = format_memory_block_with_budget(
            &selected,
            Some(input.current_session_id),
            input.budget_bytes,
        );
        MemoryBlock { content: block }
    }
}

fn collect_explicit_kinds(registry: &MemoryKindRegistry, user_query: &str) -> HashSet<String> {
    registry
        .list_definitions()
        .iter()
        .filter(|def| registry.kind_explicitly_requested(&def.id, user_query))
        .map(|def| def.id.clone())
        .collect()
}

fn explicit_statuses(def: &MemoryKindDefinition) -> Vec<MemoryStatus> {
    match def.default_status {
        MemoryStatus::Open => vec![MemoryStatus::Open],
        MemoryStatus::Active => vec![MemoryStatus::Active],
        other => vec![other],
    }
}

fn project_scope_key<'a>(
    def: &MemoryKindDefinition,
    project_key: Option<&'a str>,
) -> Option<&'a str> {
    if def.default_scope == MemoryScope::Project {
        project_key
    } else {
        None
    }
}

fn collect_kind_entries(
    input: &MemoryResolveInput<'_>,
    def: &MemoryKindDefinition,
    project_key: Option<&str>,
    statuses: &[MemoryStatus],
    inject: MemoryInjectPolicy,
) -> Vec<MemoryEntry> {
    let mut matched: Vec<&MemoryEntry> = input
        .entries
        .iter()
        .filter(|entry| {
            entry.kind == def.id
                && statuses.contains(&entry.status)
                && entry.inject == inject
                && matches_scope(entry, project_key)
        })
        .collect();
    matched.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at_ms));

    let limit = match def.cardinality {
        MemoryCardinality::SingleEffective => 1,
        MemoryCardinality::Multiple => max_entries_limit(def.prompt.max_entries),
    };
    matched
        .into_iter()
        .take(limit)
        .map(|entry| (*entry).clone())
        .collect()
}

fn max_entries_limit(max_entries: Option<u32>) -> usize {
    match max_entries {
        None | Some(0) => usize::MAX,
        Some(n) => n as usize,
    }
}

fn push_unique(
    selected: &mut Vec<MemoryEntry>,
    seen_ids: &mut HashSet<String>,
    entries: Vec<MemoryEntry>,
) {
    for entry in entries {
        if seen_ids.insert(entry.id.clone()) {
            selected.push(entry);
        }
    }
}

fn enforce_kind_limits(registry: &MemoryKindRegistry, selected: &mut Vec<MemoryEntry>) {
    let mut by_kind: HashMap<String, Vec<MemoryEntry>> = HashMap::new();
    for entry in selected.drain(..) {
        by_kind.entry(entry.kind.clone()).or_default().push(entry);
    }
    for (kind, mut entries) in by_kind {
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at_ms));
        let limit = registry
            .get(&kind)
            .map(|def| match def.cardinality {
                MemoryCardinality::SingleEffective => 1,
                MemoryCardinality::Multiple => max_entries_limit(def.prompt.max_entries),
            })
            .unwrap_or(usize::MAX);
        selected.extend(entries.into_iter().take(limit));
    }
}

fn sort_selected_by_priority(registry: &MemoryKindRegistry, selected: &mut [MemoryEntry]) {
    let priority: HashMap<&str, u32> = registry
        .list_definitions()
        .iter()
        .map(|def| (def.id.as_str(), def.prompt.priority))
        .collect();
    selected.sort_by(|left, right| {
        let left_priority = priority
            .get(left.kind.as_str())
            .copied()
            .unwrap_or(u32::MAX);
        let right_priority = priority
            .get(right.kind.as_str())
            .copied()
            .unwrap_or(u32::MAX);
        left_priority
            .cmp(&right_priority)
            .then_with(|| right.updated_at_ms.cmp(&left.updated_at_ms))
    });
}

fn entry_allowed_for_prompt(entry: &MemoryEntry, project_key: Option<&str>) -> bool {
    matches_scope(entry, project_key)
        && !matches!(
            entry.status,
            MemoryStatus::Inactive | MemoryStatus::Archived
        )
}

fn matches_scope(entry: &MemoryEntry, project_key: Option<&str>) -> bool {
    match entry.scope {
        MemoryScope::Session => true,
        MemoryScope::Project => entry
            .project_key
            .as_deref()
            .is_some_and(|pk| project_key.is_some_and(|want| pk == want)),
        MemoryScope::Global => true,
    }
}

fn entry_related_to_query(
    registry: &MemoryKindRegistry,
    user_query: &str,
    entry: &MemoryEntry,
) -> bool {
    let lower = user_query.to_lowercase();
    let text_lower = entry.text.to_lowercase();

    if let Some(def) = registry.get(&entry.kind) {
        if lower.contains(&def.id.to_lowercase()) {
            return true;
        }
        for alias in &def.aliases {
            if alias.len() >= 2 && lower.contains(&alias.to_lowercase()) {
                return true;
            }
        }
    }

    for token in lower.split_whitespace() {
        if token.len() >= 3 && text_lower.contains(token) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::contextual_memory::{
        STANDARD_KIND_GOAL, STANDARD_KIND_IDEA, STANDARD_KIND_NOW,
    };
    use crate::domain::memory_kind_registry::builtin_memory_kind_registry;
    use aibe_protocol::MEMORY_PROMPT_BUDGET_BYTES;

    fn sample_entry(kind: &str, status: MemoryStatus, text: &str) -> MemoryEntry {
        let registry = builtin_memory_kind_registry();
        let def = registry.get(kind);
        MemoryEntry {
            id: format!("mem_{kind}_{text}"),
            memory_space_id: "ctx_a".into(),
            created_session_id: "s1".into(),
            last_session_id: "s1".into(),
            kind: kind.into(),
            scope: def.map(|d| d.default_scope).unwrap_or(MemoryScope::Project),
            inject: def
                .map(|d| d.default_inject)
                .unwrap_or(MemoryInjectPolicy::Pinned),
            status,
            text: text.into(),
            project_key: if kind == STANDARD_KIND_NOW {
                None
            } else {
                Some("/proj".into())
            },
            created_at_ms: 1,
            updated_at_ms: 1,
            version: 1,
        }
    }

    fn resolve(entries: &[MemoryEntry], query: &str) -> MemoryBlock {
        let registry = builtin_memory_kind_registry();
        MemoryResolverPolicy::resolve(&MemoryResolveInput {
            entries,
            registry,
            project_key: Some("/proj"),
            current_session_id: "s1",
            user_query: query,
            budget_bytes: MEMORY_PROMPT_BUDGET_BYTES,
        })
    }

    #[test]
    fn normal_query_includes_goal_now_rule_excludes_idea() {
        let mut rule = sample_entry("rule", MemoryStatus::Active, "no fat aish");
        rule.inject = MemoryInjectPolicy::Pinned;
        let entries = vec![
            sample_entry(STANDARD_KIND_GOAL, MemoryStatus::Active, "g"),
            sample_entry(STANDARD_KIND_NOW, MemoryStatus::Active, "n"),
            rule,
            sample_entry(STANDARD_KIND_IDEA, MemoryStatus::Open, "i"),
        ];
        let block = resolve(&entries, "fix rust error");
        assert!(block.content.contains("[goal]"));
        assert!(block.content.contains("[now]"));
        assert!(block.content.contains("[rule]"));
        assert!(!block.content.contains("[idea]"));
    }

    #[test]
    fn idea_query_includes_open_ideas() {
        let entries = vec![
            sample_entry(STANDARD_KIND_GOAL, MemoryStatus::Active, "g"),
            sample_entry(STANDARD_KIND_IDEA, MemoryStatus::Open, "open idea"),
        ];
        let block = resolve(&entries, "アイデアを整理して");
        assert!(block.content.contains("[idea]"));
        assert!(block.content.contains("open idea"));
    }

    #[test]
    fn goal_mention_query_may_include_ideas() {
        let entries = vec![sample_entry(
            STANDARD_KIND_IDEA,
            MemoryStatus::Open,
            "candidate",
        )];
        let block = resolve(&entries, "ゴールを整理したい");
        assert!(block.content.contains("[idea]"));
    }

    #[test]
    fn decision_query_includes_active_decisions() {
        let entries = vec![
            sample_entry("decision", MemoryStatus::Active, "use tokio"),
            sample_entry(STANDARD_KIND_IDEA, MemoryStatus::Open, "noise"),
        ];
        let block = resolve(&entries, "決定事項を確認して");
        assert!(block.content.contains("[decision]"));
        assert!(block.content.contains("use tokio"));
        assert!(!block.content.contains("[idea]"));
    }

    #[test]
    fn policy_query_includes_rule_and_decision_by_priority() {
        let mut rule = sample_entry("rule", MemoryStatus::Active, "rule text");
        rule.inject = MemoryInjectPolicy::Pinned;
        let entries = vec![
            sample_entry("decision", MemoryStatus::Active, "decision text"),
            rule,
        ];
        let block = resolve(&entries, "方針を確認");
        assert!(block.content.contains("[rule]"));
        assert!(block.content.contains("[decision]"));
        let rule_pos = block.content.find("[rule]").unwrap();
        let decision_pos = block.content.find("[decision]").unwrap();
        assert!(rule_pos < decision_pos);
    }

    #[test]
    fn archived_and_inactive_entries_are_excluded() {
        let entries = vec![
            sample_entry("decision", MemoryStatus::Archived, "old"),
            sample_entry("decision", MemoryStatus::Inactive, "replaced"),
            sample_entry("decision", MemoryStatus::Active, "current"),
        ];
        let block = resolve(&entries, "決定を確認");
        assert!(block.content.contains("current"));
        assert!(!block.content.contains("old"));
        assert!(!block.content.contains("replaced"));
    }

    #[test]
    fn project_scope_requires_matching_project_key() {
        let mut entry = sample_entry(STANDARD_KIND_GOAL, MemoryStatus::Active, "scoped");
        entry.project_key = Some("/other".into());
        let block = resolve(&[entry], "fix bug");
        assert!(!block.content.contains("[goal]"));
    }

    #[test]
    fn global_scope_entry_matches_without_project_key() {
        let entry = MemoryEntry {
            id: "mem_global".into(),
            memory_space_id: "ctx_a".into(),
            created_session_id: "s1".into(),
            last_session_id: "s1".into(),
            kind: "custom".into(),
            scope: MemoryScope::Global,
            inject: MemoryInjectPolicy::OnDemand,
            status: MemoryStatus::Active,
            text: "global guidance".into(),
            project_key: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            version: 1,
        };
        let registry = builtin_memory_kind_registry();
        let block = MemoryResolverPolicy::resolve(&MemoryResolveInput {
            entries: std::slice::from_ref(&entry),
            registry,
            project_key: None,
            current_session_id: "s1",
            user_query: "global guidance",
            budget_bytes: MEMORY_PROMPT_BUDGET_BYTES,
        });
        assert!(block.content.contains("global guidance"));
    }

    #[test]
    fn related_selection_respects_max_entries_per_kind() {
        let mut entries: Vec<MemoryEntry> = (0..15)
            .map(|index| {
                let mut entry = sample_entry(
                    STANDARD_KIND_IDEA,
                    MemoryStatus::Open,
                    &format!("idea-{index}"),
                );
                entry.id = format!("mem_idea_{index}");
                entry.updated_at_ms = index as u64;
                entry
            })
            .collect();
        entries.push(sample_entry(STANDARD_KIND_GOAL, MemoryStatus::Active, "g"));
        let block = resolve(&entries, "アイデアを整理して");
        let idea_lines = block
            .content
            .lines()
            .filter(|line| line.starts_with("- idea-"))
            .count();
        assert_eq!(idea_lines, 12);
    }

    #[test]
    fn prompt_block_order_follows_registry_priority() {
        let mut rule = sample_entry("rule", MemoryStatus::Active, "r");
        rule.inject = MemoryInjectPolicy::Pinned;
        let entries = vec![
            rule,
            sample_entry(STANDARD_KIND_NOW, MemoryStatus::Active, "n"),
            sample_entry(STANDARD_KIND_GOAL, MemoryStatus::Active, "g"),
        ];
        let block = resolve(&entries, "query");
        let goal_pos = block.content.find("[goal]").unwrap();
        let now_pos = block.content.find("[now]").unwrap();
        let rule_pos = block.content.find("[rule]").unwrap();
        assert!(goal_pos < now_pos);
        assert!(now_pos < rule_pos);
    }

    #[test]
    fn tiny_budget_preserves_footer_without_marker() {
        use crate::domain::contextual_memory::{
            MEMORY_BLOCK_FOOTER, MEMORY_BLOCK_HEADER, MEMORY_BLOCK_TRUNCATION_MARKER,
        };
        let footer_len = MEMORY_BLOCK_FOOTER.len();
        let header_len = MEMORY_BLOCK_HEADER.len();
        let marker_with_newline_len = MEMORY_BLOCK_TRUNCATION_MARKER.len() + 1;
        let budget = header_len + footer_len;
        assert!(budget < header_len + marker_with_newline_len + footer_len);

        let entries = vec![sample_entry(
            STANDARD_KIND_GOAL,
            MemoryStatus::Active,
            "goal text that cannot fit",
        )];
        let registry = builtin_memory_kind_registry();
        let block = MemoryResolverPolicy::resolve(&MemoryResolveInput {
            entries: &entries,
            registry,
            project_key: Some("/proj"),
            current_session_id: "s1",
            user_query: "query",
            budget_bytes: budget,
        });
        assert!(block.content.len() <= budget);
        assert!(block.content.ends_with(MEMORY_BLOCK_FOOTER));
        assert!(!block.content.contains(MEMORY_BLOCK_TRUNCATION_MARKER));
    }
}
