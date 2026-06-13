//! Memory kind 定義の正本（built-in registry + user-defined merge）。

use std::collections::HashMap;
use std::sync::OnceLock;

use aibe_protocol::{
    MemoryInjectPolicyDto, MemoryKindDefinitionDto, MemoryScopeDto, MemoryStatusDto,
};
use thiserror::Error;

use super::contextual_memory::{
    validate_kind, MemoryInjectPolicy, MemoryScope, MemoryStatus, MemoryValidationError,
};

/// `kinds.toml` から読み込んだ 1 kind 分の override（未指定フィールドは `None`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KindOverride {
    pub description: Option<String>,
    pub default_scope: Option<MemoryScope>,
    pub default_inject: Option<MemoryInjectPolicy>,
    pub default_status: Option<MemoryStatus>,
    pub lifecycle: Option<MemoryLifecycle>,
    pub cardinality: Option<MemoryCardinality>,
    pub clear_from: Option<MemoryStatus>,
    pub clear_to: Option<MemoryStatus>,
    pub prompt: Option<PromptOverride>,
    pub stale: Option<MemoryStalePolicy>,
    pub dedicated_cli: Option<Option<String>>,
    pub aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptOverride {
    pub auto_inject: Option<bool>,
    pub on_demand: Option<bool>,
    pub priority: Option<u32>,
    pub keywords: Option<Vec<String>>,
    pub max_entries: Option<Option<u32>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MemoryKindRegistryError {
    #[error("kind registry io: {0}")]
    Io(String),
    #[error("kind registry parse: {0}")]
    Parse(String),
    #[error("kind registry override forbidden for builtin kind {kind}: {reason}")]
    BuiltinOverrideForbidden { kind: String, reason: String },
    #[error("kind registry custom kind {kind} missing required field: {field}")]
    CustomKindMissingField { kind: String, field: String },
    #[error("kind registry duplicate kind id: {0}")]
    DuplicateKindId(String),
}

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
        self.kind_explicitly_requested(kind, user_query)
    }

    /// query が kind id / alias / on_demand keywords のいずれかを含むか。
    pub fn kind_explicitly_requested(&self, kind: &str, user_query: &str) -> bool {
        let Some(def) = self.get(kind) else {
            return false;
        };
        let lower = user_query.to_lowercase();
        if lower.contains(&def.id.to_lowercase()) {
            return true;
        }
        for alias in &def.aliases {
            if lower.contains(&alias.to_lowercase()) {
                return true;
            }
        }
        if def.prompt.on_demand {
            for kw in &def.prompt.keywords {
                if lower.contains(kw) {
                    return true;
                }
            }
        }
        false
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

    /// built-in を複製した registry（filesystem override merge の起点）。
    pub fn from_builtin() -> Self {
        Self::builtin()
    }

    /// server / memory-space-local の override を後勝ちで merge する。
    pub fn merge_overrides(
        &mut self,
        overrides: &HashMap<String, KindOverride>,
    ) -> Result<(), MemoryKindRegistryError> {
        for (id, ov) in overrides {
            validate_kind(id).map_err(|e| MemoryKindRegistryError::Parse(e.to_string()))?;
            if let Some(existing) = self.kinds.get(id) {
                if existing.builtin {
                    self.apply_builtin_override(id, ov)?;
                } else {
                    self.apply_custom_override(id, ov)?;
                }
            } else {
                self.insert_custom_kind(id, ov)?;
            }
        }
        Ok(())
    }

    fn apply_builtin_override(
        &mut self,
        id: &str,
        ov: &KindOverride,
    ) -> Result<(), MemoryKindRegistryError> {
        let base = self.kinds.get(id).expect("builtin kind").clone();
        validate_builtin_override(&base, ov)?;
        let def = self.kinds.get_mut(id).expect("builtin kind");
        if let Some(description) = &ov.description {
            def.description = description.clone();
        }
        if let Some(aliases) = &ov.aliases {
            def.aliases = aliases.clone();
        }
        if let Some(stale) = ov.stale {
            def.stale = stale;
        }
        if let Some(prompt_ov) = &ov.prompt {
            apply_builtin_prompt_override(&mut def.prompt, prompt_ov);
        }
        Ok(())
    }

    fn apply_custom_override(
        &mut self,
        id: &str,
        ov: &KindOverride,
    ) -> Result<(), MemoryKindRegistryError> {
        let base = self.kinds.get(id).expect("custom kind").clone();
        let merged = merge_kind_definition(&base, ov);
        self.kinds.insert(id.to_string(), merged);
        Ok(())
    }

    fn insert_custom_kind(
        &mut self,
        id: &str,
        ov: &KindOverride,
    ) -> Result<(), MemoryKindRegistryError> {
        let def = kind_from_override(id, ov)?;
        if self.kinds.contains_key(id) {
            return Err(MemoryKindRegistryError::DuplicateKindId(id.to_string()));
        }
        self.kinds.insert(id.to_string(), def);
        Ok(())
    }
}

fn apply_builtin_prompt_override(prompt: &mut MemoryPromptPolicy, ov: &PromptOverride) {
    if let Some(priority) = ov.priority {
        prompt.priority = priority;
    }
    if let Some(keywords) = &ov.keywords {
        prompt.keywords = keywords.clone();
    }
    if let Some(max_entries) = ov.max_entries {
        prompt.max_entries = max_entries;
    }
}

fn apply_prompt_override(prompt: &mut MemoryPromptPolicy, ov: &PromptOverride) {
    if let Some(auto_inject) = ov.auto_inject {
        prompt.auto_inject = auto_inject;
    }
    if let Some(on_demand) = ov.on_demand {
        prompt.on_demand = on_demand;
    }
    if let Some(priority) = ov.priority {
        prompt.priority = priority;
    }
    if let Some(keywords) = &ov.keywords {
        prompt.keywords = keywords.clone();
    }
    if let Some(max_entries) = ov.max_entries {
        prompt.max_entries = max_entries;
    }
}

fn merge_kind_definition(base: &MemoryKindDefinition, ov: &KindOverride) -> MemoryKindDefinition {
    let mut def = base.clone();
    if let Some(description) = &ov.description {
        def.description = description.clone();
    }
    if let Some(scope) = ov.default_scope {
        def.default_scope = scope;
    }
    if let Some(inject) = ov.default_inject {
        def.default_inject = inject;
    }
    if let Some(status) = ov.default_status {
        def.default_status = status;
    }
    if let Some(lifecycle) = ov.lifecycle {
        def.lifecycle = lifecycle;
    }
    if let Some(cardinality) = ov.cardinality {
        def.cardinality = cardinality;
    }
    if let Some(clear_from) = ov.clear_from {
        def.clear_from = clear_from;
    }
    if let Some(clear_to) = ov.clear_to {
        def.clear_to = clear_to;
    }
    if let Some(stale) = ov.stale {
        def.stale = stale;
    }
    if let Some(dedicated_cli) = &ov.dedicated_cli {
        def.dedicated_cli = dedicated_cli.clone();
    }
    if let Some(aliases) = &ov.aliases {
        def.aliases = aliases.clone();
    }
    if let Some(prompt_ov) = &ov.prompt {
        apply_prompt_override(&mut def.prompt, prompt_ov);
    }
    def
}

fn kind_from_override(
    id: &str,
    ov: &KindOverride,
) -> Result<MemoryKindDefinition, MemoryKindRegistryError> {
    macro_rules! require {
        ($field:expr, $name:literal) => {
            $field.ok_or_else(|| MemoryKindRegistryError::CustomKindMissingField {
                kind: id.to_string(),
                field: $name.to_string(),
            })?
        };
    }
    let description = require!(ov.description.clone(), "description");
    let default_scope = require!(ov.default_scope, "default_scope");
    let default_inject = require!(ov.default_inject, "default_inject");
    let default_status = require!(ov.default_status, "default_status");
    let lifecycle = require!(ov.lifecycle, "lifecycle");
    let cardinality = require!(ov.cardinality, "cardinality");
    let clear_from = require!(ov.clear_from, "clear_from");
    let clear_to = require!(ov.clear_to, "clear_to");
    let prompt = ov
        .prompt
        .as_ref()
        .map(|p| MemoryPromptPolicy {
            auto_inject: p.auto_inject.unwrap_or(false),
            on_demand: p.on_demand.unwrap_or(false),
            priority: p.priority.unwrap_or(100),
            keywords: p.keywords.clone().unwrap_or_default(),
            max_entries: p.max_entries.unwrap_or(Some(0)),
        })
        .unwrap_or(MemoryPromptPolicy {
            auto_inject: false,
            on_demand: false,
            priority: 100,
            keywords: vec![],
            max_entries: Some(0),
        });
    Ok(MemoryKindDefinition {
        id: id.to_string(),
        description,
        default_scope,
        default_inject,
        default_status,
        lifecycle,
        cardinality,
        clear_from,
        clear_to,
        prompt,
        stale: ov.stale.unwrap_or(MemoryStalePolicy::None),
        builtin: false,
        dedicated_cli: ov.dedicated_cli.clone().unwrap_or(None),
        aliases: ov.aliases.clone().unwrap_or_default(),
    })
}

fn validate_builtin_override(
    base: &MemoryKindDefinition,
    ov: &KindOverride,
) -> Result<(), MemoryKindRegistryError> {
    let forbidden = |reason: &str| MemoryKindRegistryError::BuiltinOverrideForbidden {
        kind: base.id.clone(),
        reason: reason.to_string(),
    };
    if let Some(scope) = ov.default_scope {
        if scope != base.default_scope {
            return Err(forbidden(
                "default_scope cannot be changed for builtin kind",
            ));
        }
    }
    if let Some(inject) = ov.default_inject {
        if inject != base.default_inject {
            return Err(forbidden(
                "default_inject cannot be changed for builtin kind",
            ));
        }
    }
    if let Some(status) = ov.default_status {
        if status != base.default_status {
            return Err(forbidden(
                "default_status cannot be changed for builtin kind",
            ));
        }
    }
    if let Some(lifecycle) = ov.lifecycle {
        if lifecycle != base.lifecycle {
            return Err(forbidden("lifecycle cannot be changed for builtin kind"));
        }
    }
    if let Some(cardinality) = ov.cardinality {
        if cardinality != base.cardinality {
            return Err(forbidden("cardinality cannot be changed for builtin kind"));
        }
    }
    if let Some(clear_from) = ov.clear_from {
        if clear_from != base.clear_from {
            return Err(forbidden("clear_from cannot be changed for builtin kind"));
        }
    }
    if let Some(clear_to) = ov.clear_to {
        if clear_to != base.clear_to {
            return Err(forbidden("clear_to cannot be changed for builtin kind"));
        }
    }
    if base.id == "goal" && ov.cardinality == Some(MemoryCardinality::Multiple) {
        return Err(forbidden("goal cannot be multiple"));
    }
    if base.id == "now" && ov.default_scope == Some(MemoryScope::Project) {
        return Err(forbidden("now cannot be project scope"));
    }
    if base.id == "idea" {
        let auto_inject = ov
            .prompt
            .as_ref()
            .and_then(|p| p.auto_inject)
            .unwrap_or(base.prompt.auto_inject);
        let inject = ov.default_inject.unwrap_or(base.default_inject);
        if auto_inject && inject == MemoryInjectPolicy::Pinned {
            return Err(forbidden("idea cannot be pinned auto-inject"));
        }
        if ov.default_inject == Some(MemoryInjectPolicy::Pinned) && base.prompt.auto_inject {
            return Err(forbidden("idea cannot be pinned auto-inject"));
        }
    }
    if let Some(prompt_ov) = &ov.prompt {
        if prompt_ov.auto_inject.is_some() {
            return Err(forbidden(
                "prompt.auto_inject cannot be changed for builtin kind",
            ));
        }
        if prompt_ov.on_demand.is_some() {
            return Err(forbidden(
                "prompt.on_demand cannot be changed for builtin kind",
            ));
        }
    }
    Ok(())
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
    fn decision_explicit_alias_matches() {
        let reg = registry();
        assert!(reg.kind_explicitly_requested("decision", "決定事項を確認"));
        assert!(reg.kind_explicitly_requested("decision", "方針を確認"));
        assert!(!reg.kind_explicitly_requested("decision", "fix rust error"));
    }

    #[test]
    fn rule_explicit_alias_matches_policy() {
        let reg = registry();
        assert!(reg.kind_explicitly_requested("rule", "ルールを確認"));
        assert!(reg.kind_explicitly_requested("rule", "方針を確認"));
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

    #[test]
    fn builtin_override_allows_aliases_and_priority() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "goal".into(),
            KindOverride {
                description: Some("team goal".into()),
                aliases: Some(vec!["goal".into(), "チーム目標".into()]),
                prompt: Some(PromptOverride {
                    priority: Some(5),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        reg.merge_overrides(&overrides).expect("merge");
        let def = reg.get("goal").unwrap();
        assert_eq!(def.description, "team goal");
        assert_eq!(def.prompt.priority, 5);
        assert!(def.aliases.contains(&"チーム目標".into()));
    }

    #[test]
    fn builtin_override_rejects_goal_multiple() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "goal".into(),
            KindOverride {
                cardinality: Some(MemoryCardinality::Multiple),
                ..Default::default()
            },
        );
        assert!(matches!(
            reg.merge_overrides(&overrides),
            Err(MemoryKindRegistryError::BuiltinOverrideForbidden { kind, .. }) if kind == "goal"
        ));
    }

    #[test]
    fn builtin_override_rejects_idea_pinned_auto_inject() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "idea".into(),
            KindOverride {
                default_inject: Some(MemoryInjectPolicy::Pinned),
                ..Default::default()
            },
        );
        assert!(matches!(
            reg.merge_overrides(&overrides),
            Err(MemoryKindRegistryError::BuiltinOverrideForbidden { kind, .. }) if kind == "idea"
        ));
    }

    #[test]
    fn builtin_override_rejects_prompt_auto_inject_change() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "goal".into(),
            KindOverride {
                prompt: Some(PromptOverride {
                    auto_inject: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        assert!(matches!(
            reg.merge_overrides(&overrides),
            Err(MemoryKindRegistryError::BuiltinOverrideForbidden { kind, .. }) if kind == "goal"
        ));
    }

    #[test]
    fn builtin_override_rejects_prompt_on_demand_change() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "idea".into(),
            KindOverride {
                prompt: Some(PromptOverride {
                    on_demand: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        assert!(matches!(
            reg.merge_overrides(&overrides),
            Err(MemoryKindRegistryError::BuiltinOverrideForbidden { kind, .. }) if kind == "idea"
        ));
    }

    #[test]
    fn builtin_override_allows_keywords_and_priority() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "idea".into(),
            KindOverride {
                prompt: Some(PromptOverride {
                    priority: Some(70),
                    keywords: Some(vec!["custom-kw".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        reg.merge_overrides(&overrides).expect("merge");
        let def = reg.get("idea").unwrap();
        assert_eq!(def.prompt.priority, 70);
        assert!(def.prompt.keywords.contains(&"custom-kw".into()));
        assert!(!def.prompt.auto_inject);
        assert!(def.prompt.on_demand);
    }

    #[test]
    fn merge_rejects_invalid_kind_id() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "bad kind".into(),
            KindOverride {
                description: Some("x".into()),
                default_scope: Some(MemoryScope::Project),
                default_inject: Some(MemoryInjectPolicy::Manual),
                default_status: Some(MemoryStatus::Open),
                lifecycle: Some(MemoryLifecycle::OpenArchive),
                cardinality: Some(MemoryCardinality::Multiple),
                clear_from: Some(MemoryStatus::Open),
                clear_to: Some(MemoryStatus::Archived),
                ..Default::default()
            },
        );
        assert!(reg.merge_overrides(&overrides).is_err());
    }

    #[test]
    fn custom_kind_from_toml_override() {
        let mut reg = registry();
        let mut overrides = HashMap::new();
        overrides.insert(
            "checklist".into(),
            KindOverride {
                description: Some("チェックリスト".into()),
                default_scope: Some(MemoryScope::Project),
                default_inject: Some(MemoryInjectPolicy::Manual),
                default_status: Some(MemoryStatus::Open),
                lifecycle: Some(MemoryLifecycle::OpenArchive),
                cardinality: Some(MemoryCardinality::Multiple),
                clear_from: Some(MemoryStatus::Open),
                clear_to: Some(MemoryStatus::Archived),
                aliases: Some(vec!["checklist".into()]),
                ..Default::default()
            },
        );
        reg.merge_overrides(&overrides).expect("merge");
        assert!(reg.is_registered("checklist"));
    }
}
