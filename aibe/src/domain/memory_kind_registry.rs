//! Memory kind 定義の正本（built-in registry）。

use std::collections::HashMap;
use std::sync::OnceLock;

use aibe_protocol::{
    MemoryInjectPolicyDto, MemoryKindDefinitionDto, MemoryScopeDto, MemoryStatusDto,
};

use super::contextual_memory::{
    MemoryInjectPolicy, MemoryScope, MemoryStatus, MemoryValidationError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLifecycle {
    ActiveInactive,
    OpenArchive,
    ActiveArchive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCardinality {
    SingleEffective,
    Multiple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStalePolicy {
    None,
    SessionChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPromptPolicy {
    pub auto_inject: bool,
    pub on_demand: bool,
    pub priority: u32,
    pub keywords: Vec<String>,
    pub max_entries: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryKindDefinition {
    pub id: String,
    pub description: String,
    pub default_scope: MemoryScope,
    pub default_inject: MemoryInjectPolicy,
    pub default_status: MemoryStatus,
    pub lifecycle: MemoryLifecycle,
    pub cardinality: MemoryCardinality,
    pub clear_from: MemoryStatus,
    pub clear_to: MemoryStatus,
    pub prompt: MemoryPromptPolicy,
    pub stale: MemoryStalePolicy,
    pub builtin: bool,
    pub dedicated_cli: Option<String>,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryKindRegistry {
    kinds: HashMap<String, MemoryKindDefinition>,
}

impl MemoryKindRegistry {
    pub fn builtin() -> Self {
        let defs = [
            goal_definition(),
            now_definition(),
            rule_definition(),
            decision_definition(),
            idea_definition(),
            note_definition(),
        ];
        let mut kinds = HashMap::new();
        for def in defs {
            kinds.insert(def.id.clone(), def);
        }
        Self { kinds }
    }

    pub fn get(&self, kind: &str) -> Option<&MemoryKindDefinition> {
        self.kinds.get(kind)
    }

    pub fn is_registered(&self, kind: &str) -> bool {
        self.kinds.contains_key(kind)
    }

    pub fn has_dedicated_cli(&self, kind: &str) -> bool {
        self.get(kind)
            .and_then(|d| d.dedicated_cli.as_ref())
            .is_some()
    }

    pub fn pinned_auto_inject_definitions(&self) -> Vec<&MemoryKindDefinition> {
        let mut defs: Vec<_> = self
            .kinds
            .values()
            .filter(|d| d.prompt.auto_inject && d.default_inject == MemoryInjectPolicy::Pinned)
            .collect();
        defs.sort_by_key(|d| d.prompt.priority);
        defs
    }

    pub fn on_demand_definitions(&self) -> Vec<&MemoryKindDefinition> {
        let mut defs: Vec<_> = self
            .kinds
            .values()
            .filter(|d| d.prompt.on_demand && d.default_inject == MemoryInjectPolicy::OnDemand)
            .collect();
        defs.sort_by_key(|d| d.prompt.priority);
        defs
    }

    pub fn validate_operation(
        &self,
        kind: &str,
        scope: MemoryScopeDto,
        inject: MemoryInjectPolicyDto,
        status: MemoryStatusDto,
    ) -> Result<(), MemoryValidationError> {
        let Some(def) = self.get(kind) else {
            return Ok(());
        };
        let ok = MemoryScopeDto::from(def.default_scope) == scope
            && MemoryInjectPolicyDto::from(def.default_inject) == inject
            && MemoryStatusDto::from(def.default_status) == status;
        if ok {
            Ok(())
        } else {
            Err(MemoryValidationError::StandardKindMismatch {
                kind: kind.to_string(),
            })
        }
    }

    pub fn clear_transition(&self, kind: &str) -> (MemoryStatus, MemoryStatus) {
        self.get(kind)
            .map(|d| (d.clear_from, d.clear_to))
            .unwrap_or((MemoryStatus::Open, MemoryStatus::Archived))
    }

    pub fn query_matches_on_demand(&self, kind: &str, user_query: &str) -> bool {
        let Some(def) = self.get(kind) else {
            return false;
        };
        if !def.prompt.on_demand {
            return false;
        }
        let lower = user_query.to_lowercase();
        def.prompt.keywords.iter().any(|kw| lower.contains(kw))
    }

    pub fn is_list_format_kind(&self, kind: &str) -> bool {
        self.get(kind)
            .is_some_and(|d| d.lifecycle == MemoryLifecycle::OpenArchive)
    }

    pub fn stale_policy(&self, kind: &str) -> MemoryStalePolicy {
        self.get(kind)
            .map(|d| d.stale)
            .unwrap_or(MemoryStalePolicy::None)
    }

    pub fn list_definitions(&self) -> Vec<&MemoryKindDefinition> {
        let mut defs: Vec<_> = self.kinds.values().collect();
        defs.sort_by_key(|d| d.prompt.priority);
        defs
    }
}

pub fn builtin_memory_kind_registry() -> &'static MemoryKindRegistry {
    static REGISTRY: OnceLock<MemoryKindRegistry> = OnceLock::new();
    REGISTRY.get_or_init(MemoryKindRegistry::builtin)
}

impl From<&MemoryKindDefinition> for MemoryKindDefinitionDto {
    fn from(def: &MemoryKindDefinition) -> Self {
        Self {
            id: def.id.clone(),
            description: def.description.clone(),
            default_scope: MemoryScopeDto::from(def.default_scope),
            default_inject: MemoryInjectPolicyDto::from(def.default_inject),
            default_status: MemoryStatusDto::from(def.default_status),
            lifecycle: lifecycle_to_wire(def.lifecycle),
            cardinality: cardinality_to_wire(def.cardinality),
            clear_from: MemoryStatusDto::from(def.clear_from),
            clear_to: MemoryStatusDto::from(def.clear_to),
            auto_inject: def.prompt.auto_inject,
            on_demand: def.prompt.on_demand,
            priority: def.prompt.priority,
            keywords: def.prompt.keywords.clone(),
            max_entries: def.prompt.max_entries,
            aliases: def.aliases.clone(),
            builtin: def.builtin,
            dedicated_cli: def.dedicated_cli.clone(),
        }
    }
}

fn lifecycle_to_wire(lifecycle: MemoryLifecycle) -> String {
    match lifecycle {
        MemoryLifecycle::ActiveInactive => "active_inactive".into(),
        MemoryLifecycle::OpenArchive => "open_archive".into(),
        MemoryLifecycle::ActiveArchive => "active_archive".into(),
    }
}

fn cardinality_to_wire(cardinality: MemoryCardinality) -> String {
    match cardinality {
        MemoryCardinality::SingleEffective => "single_effective".into(),
        MemoryCardinality::Multiple => "multiple".into(),
    }
}

fn goal_definition() -> MemoryKindDefinition {
    MemoryKindDefinition {
        id: "goal".into(),
        description: "作業の最終目的".into(),
        default_scope: MemoryScope::Project,
        default_inject: MemoryInjectPolicy::Pinned,
        default_status: MemoryStatus::Active,
        lifecycle: MemoryLifecycle::ActiveInactive,
        cardinality: MemoryCardinality::SingleEffective,
        clear_from: MemoryStatus::Active,
        clear_to: MemoryStatus::Inactive,
        prompt: MemoryPromptPolicy {
            auto_inject: true,
            on_demand: false,
            priority: 10,
            keywords: vec![],
            max_entries: Some(1),
        },
        stale: MemoryStalePolicy::None,
        builtin: true,
        dedicated_cli: Some("ai goal set".into()),
        aliases: vec![
            "goal".into(),
            "目的".into(),
            "ゴール".into(),
            "最終目的".into(),
        ],
    }
}

fn now_definition() -> MemoryKindDefinition {
    MemoryKindDefinition {
        id: "now".into(),
        description: "現在の焦点".into(),
        default_scope: MemoryScope::Session,
        default_inject: MemoryInjectPolicy::Pinned,
        default_status: MemoryStatus::Active,
        lifecycle: MemoryLifecycle::ActiveInactive,
        cardinality: MemoryCardinality::SingleEffective,
        clear_from: MemoryStatus::Active,
        clear_to: MemoryStatus::Inactive,
        prompt: MemoryPromptPolicy {
            auto_inject: true,
            on_demand: false,
            priority: 20,
            keywords: vec![],
            max_entries: Some(1),
        },
        stale: MemoryStalePolicy::SessionChanged,
        builtin: true,
        dedicated_cli: Some("ai now set".into()),
        aliases: vec![
            "now".into(),
            "focus".into(),
            "現在".into(),
            "焦点".into(),
            "今やること".into(),
        ],
    }
}

fn rule_definition() -> MemoryKindDefinition {
    MemoryKindDefinition {
        id: "rule".into(),
        description: "ユーザーが明示した作業ルール".into(),
        default_scope: MemoryScope::Project,
        default_inject: MemoryInjectPolicy::Pinned,
        default_status: MemoryStatus::Active,
        lifecycle: MemoryLifecycle::ActiveArchive,
        cardinality: MemoryCardinality::Multiple,
        clear_from: MemoryStatus::Active,
        clear_to: MemoryStatus::Archived,
        prompt: MemoryPromptPolicy {
            auto_inject: true,
            on_demand: false,
            priority: 30,
            keywords: vec![],
            max_entries: Some(8),
        },
        stale: MemoryStalePolicy::None,
        builtin: true,
        dedicated_cli: None,
        aliases: vec![
            "rule".into(),
            "rules".into(),
            "ルール".into(),
            "制約".into(),
            "方針".into(),
        ],
    }
}

fn decision_definition() -> MemoryKindDefinition {
    MemoryKindDefinition {
        id: "decision".into(),
        description: "決定済み事項".into(),
        default_scope: MemoryScope::Project,
        default_inject: MemoryInjectPolicy::OnDemand,
        default_status: MemoryStatus::Active,
        lifecycle: MemoryLifecycle::ActiveArchive,
        cardinality: MemoryCardinality::Multiple,
        clear_from: MemoryStatus::Active,
        clear_to: MemoryStatus::Archived,
        prompt: MemoryPromptPolicy {
            auto_inject: false,
            on_demand: true,
            priority: 60,
            keywords: vec![],
            max_entries: Some(8),
        },
        stale: MemoryStalePolicy::None,
        builtin: true,
        dedicated_cli: None,
        aliases: vec![
            "decision".into(),
            "decisions".into(),
            "決定".into(),
            "決定事項".into(),
            "採用".into(),
            "方針".into(),
        ],
    }
}

fn idea_definition() -> MemoryKindDefinition {
    MemoryKindDefinition {
        id: "idea".into(),
        description: "未整理のアイデア".into(),
        default_scope: MemoryScope::Project,
        default_inject: MemoryInjectPolicy::OnDemand,
        default_status: MemoryStatus::Open,
        lifecycle: MemoryLifecycle::OpenArchive,
        cardinality: MemoryCardinality::Multiple,
        clear_from: MemoryStatus::Open,
        clear_to: MemoryStatus::Archived,
        prompt: MemoryPromptPolicy {
            auto_inject: false,
            on_demand: true,
            priority: 80,
            keywords: vec![
                "idea".into(),
                "ideas".into(),
                "アイデア".into(),
                "発想".into(),
                "ゴール".into(),
                "goal".into(),
                "整理".into(),
                "候補".into(),
                "mvp".into(),
                "未整理".into(),
                "記憶".into(),
                "memory".into(),
            ],
            max_entries: Some(12),
        },
        stale: MemoryStalePolicy::None,
        builtin: true,
        dedicated_cli: Some("ai idea add".into()),
        aliases: vec![
            "idea".into(),
            "ideas".into(),
            "アイデア".into(),
            "発想".into(),
            "候補".into(),
            "未整理".into(),
        ],
    }
}

fn note_definition() -> MemoryKindDefinition {
    MemoryKindDefinition {
        id: "note".into(),
        description: "汎用メモ".into(),
        default_scope: MemoryScope::Project,
        default_inject: MemoryInjectPolicy::Manual,
        default_status: MemoryStatus::Open,
        lifecycle: MemoryLifecycle::OpenArchive,
        cardinality: MemoryCardinality::Multiple,
        clear_from: MemoryStatus::Open,
        clear_to: MemoryStatus::Archived,
        prompt: MemoryPromptPolicy {
            auto_inject: false,
            on_demand: false,
            priority: 100,
            keywords: vec![],
            max_entries: Some(0),
        },
        stale: MemoryStalePolicy::None,
        builtin: true,
        dedicated_cli: None,
        aliases: vec!["note".into(), "memo".into(), "メモ".into(), "ノート".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> MemoryKindRegistry {
        MemoryKindRegistry::builtin()
    }

    #[test]
    fn goal_is_project_pinned_active_single_effective() {
        let reg = registry();
        let def = reg.get("goal").unwrap();
        assert_eq!(def.default_scope, MemoryScope::Project);
        assert_eq!(def.default_inject, MemoryInjectPolicy::Pinned);
        assert_eq!(def.default_status, MemoryStatus::Active);
        assert_eq!(def.cardinality, MemoryCardinality::SingleEffective);
        assert_eq!(def.prompt.priority, 10);
        assert_eq!(def.prompt.max_entries, Some(1));
    }

    #[test]
    fn now_is_session_pinned_active_with_stale() {
        let reg = registry();
        let def = reg.get("now").unwrap();
        assert_eq!(def.default_scope, MemoryScope::Session);
        assert_eq!(def.default_inject, MemoryInjectPolicy::Pinned);
        assert_eq!(def.stale, MemoryStalePolicy::SessionChanged);
        assert_eq!(def.prompt.priority, 20);
    }

    #[test]
    fn idea_is_project_on_demand_open() {
        let reg = registry();
        let def = reg.get("idea").unwrap();
        assert_eq!(def.default_scope, MemoryScope::Project);
        assert_eq!(def.default_inject, MemoryInjectPolicy::OnDemand);
        assert_eq!(def.default_status, MemoryStatus::Open);
        assert_eq!(def.lifecycle, MemoryLifecycle::OpenArchive);
        assert!(def.prompt.on_demand);
        assert!(!def.prompt.auto_inject);
    }

    #[test]
    fn rule_is_project_pinned_active_multiple() {
        let reg = registry();
        let def = reg.get("rule").unwrap();
        assert_eq!(def.default_scope, MemoryScope::Project);
        assert_eq!(def.default_inject, MemoryInjectPolicy::Pinned);
        assert_eq!(def.cardinality, MemoryCardinality::Multiple);
        assert_eq!(def.prompt.priority, 30);
        assert!(def.prompt.auto_inject);
    }

    #[test]
    fn decision_is_on_demand_active_multiple() {
        let reg = registry();
        let def = reg.get("decision").unwrap();
        assert_eq!(def.default_inject, MemoryInjectPolicy::OnDemand);
        assert!(def.prompt.on_demand);
        assert!(!def.prompt.auto_inject);
    }

    #[test]
    fn note_is_manual_open_unlimited() {
        let reg = registry();
        let def = reg.get("note").unwrap();
        assert_eq!(def.default_inject, MemoryInjectPolicy::Manual);
        assert_eq!(def.default_status, MemoryStatus::Open);
        assert_eq!(def.prompt.max_entries, Some(0));
    }

    #[test]
    fn pinned_auto_inject_order_is_goal_now_rule() {
        let reg = registry();
        let ids: Vec<_> = reg
            .pinned_auto_inject_definitions()
            .iter()
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(ids, vec!["goal", "now", "rule"]);
    }

    #[test]
    fn clear_transition_uses_registry() {
        let reg = registry();
        assert_eq!(
            reg.clear_transition("goal"),
            (MemoryStatus::Active, MemoryStatus::Inactive)
        );
        assert_eq!(
            reg.clear_transition("idea"),
            (MemoryStatus::Open, MemoryStatus::Archived)
        );
        assert_eq!(
            reg.clear_transition("rule"),
            (MemoryStatus::Active, MemoryStatus::Archived)
        );
    }

    #[test]
    fn idea_on_demand_keywords_match() {
        let reg = registry();
        assert!(reg.query_matches_on_demand("idea", "アイデアを整理して"));
        assert!(!reg.query_matches_on_demand("idea", "fix rust error"));
    }

    #[test]
    fn validate_operation_rejects_mismatch() {
        let reg = registry();
        assert!(reg
            .validate_operation(
                "goal",
                MemoryScopeDto::Session,
                MemoryInjectPolicyDto::Pinned,
                MemoryStatusDto::Active,
            )
            .is_err());
        assert!(reg
            .validate_operation(
                "goal",
                MemoryScopeDto::Project,
                MemoryInjectPolicyDto::Pinned,
                MemoryStatusDto::Active,
            )
            .is_ok());
    }
}
