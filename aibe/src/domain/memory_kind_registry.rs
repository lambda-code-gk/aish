//! Memory kind 定義の正本（TOML pack + user-defined merge）。

use std::collections::HashMap;
use std::sync::OnceLock;

use aibe_protocol::{
    MemoryInjectPolicyDto, MemoryKindDefinitionDto, MemoryScopeDto, MemoryStatusDto,
};
use serde::Deserialize;
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
    pub fn empty() -> Self {
        Self {
            kinds: HashMap::new(),
        }
    }

    /// AISH baseline pack の kinds.toml パス（crate 同梱）。
    pub fn baseline_pack_kinds_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("memory/packs/aish-memory/kinds.toml")
    }

    /// override map から registry を構築する（TOML 読み込みの共通入口）。
    pub fn from_overrides(
        overrides: &HashMap<String, KindOverride>,
        mark_builtin: bool,
    ) -> Result<Self, MemoryKindRegistryError> {
        let mut kinds = HashMap::new();
        for (id, ov) in overrides {
            validate_kind(id).map_err(|e| MemoryKindRegistryError::Parse(e.to_string()))?;
            let mut def = kind_from_override(id, ov)?;
            def.builtin = mark_builtin;
            kinds.insert(id.clone(), def);
        }
        Ok(Self { kinds })
    }

    /// TOML 文字列から full definition を読み込む。
    pub fn load_from_str(
        raw: &str,
        source: &str,
        mark_builtin: bool,
    ) -> Result<Self, MemoryKindRegistryError> {
        let overrides = parse_kinds_toml_str(raw, source)?;
        Self::from_overrides(&overrides, mark_builtin)
    }

    /// 同梱 baseline pack を読み込む（compile-time embed、I/O なし）。
    pub fn baseline() -> Result<Self, MemoryKindRegistryError> {
        const BASELINE_KINDS_TOML: &str = include_str!("../../memory/packs/aish-memory/kinds.toml");
        Self::load_from_str(BASELINE_KINDS_TOML, "aish-memory/kinds.toml", true)
    }

    /// 他 registry の kind を後勝ちで取り込む。
    pub fn merge(&mut self, other: Self) {
        for (id, def) in other.kinds {
            self.kinds.insert(id, def);
        }
    }

    pub fn merge_registry(&mut self, other: &Self) {
        for (id, def) in &other.kinds {
            self.kinds.insert(id.clone(), def.clone());
        }
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

    /// baseline AISH pack を読み込んだ registry（テスト・prompt 解決のフォールバック用）。
    pub fn from_builtin() -> Self {
        Self::baseline().expect("baseline AISH memory pack must load")
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

#[derive(Debug, Deserialize)]
struct KindsTomlRoot {
    #[serde(default)]
    kinds: HashMap<String, KindTomlEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct KindTomlEntry {
    description: Option<String>,
    default_scope: Option<String>,
    default_inject: Option<String>,
    default_status: Option<String>,
    lifecycle: Option<String>,
    cardinality: Option<String>,
    clear_from: Option<String>,
    clear_to: Option<String>,
    stale: Option<String>,
    dedicated_cli: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    prompt: Option<PromptTomlEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct PromptTomlEntry {
    auto_inject: Option<bool>,
    on_demand: Option<bool>,
    priority: Option<u32>,
    #[serde(default)]
    keywords: Vec<String>,
    max_entries: Option<u32>,
}

pub(crate) fn parse_kinds_toml_str(
    raw: &str,
    source: &str,
) -> Result<HashMap<String, KindOverride>, MemoryKindRegistryError> {
    let root: KindsTomlRoot = toml::from_str(raw)
        .map_err(|e| MemoryKindRegistryError::Parse(format!("{source}: {e}")))?;
    let mut out = HashMap::new();
    for (id, entry) in root.kinds {
        out.insert(id, entry_to_override(&entry)?);
    }
    Ok(out)
}

fn entry_to_override(entry: &KindTomlEntry) -> Result<KindOverride, MemoryKindRegistryError> {
    let aliases = if entry.aliases.is_empty() {
        None
    } else {
        Some(entry.aliases.clone())
    };
    let prompt = entry.prompt.as_ref().map(|p| {
        let keywords = if p.keywords.is_empty() {
            None
        } else {
            Some(p.keywords.clone())
        };
        PromptOverride {
            auto_inject: p.auto_inject,
            on_demand: p.on_demand,
            priority: p.priority,
            keywords,
            max_entries: p.max_entries.map(Some),
        }
    });
    Ok(KindOverride {
        description: entry.description.clone(),
        default_scope: entry
            .default_scope
            .as_deref()
            .map(parse_scope)
            .transpose()?,
        default_inject: entry
            .default_inject
            .as_deref()
            .map(parse_inject)
            .transpose()?,
        default_status: entry
            .default_status
            .as_deref()
            .map(parse_status)
            .transpose()?,
        lifecycle: entry
            .lifecycle
            .as_deref()
            .map(parse_lifecycle)
            .transpose()?,
        cardinality: entry
            .cardinality
            .as_deref()
            .map(parse_cardinality)
            .transpose()?,
        clear_from: entry.clear_from.as_deref().map(parse_status).transpose()?,
        clear_to: entry.clear_to.as_deref().map(parse_status).transpose()?,
        prompt,
        stale: entry.stale.as_deref().map(parse_stale).transpose()?,
        dedicated_cli: entry.dedicated_cli.as_ref().map(|s| Some(s.clone())),
        aliases,
    })
}

fn parse_scope(raw: &str) -> Result<MemoryScope, MemoryKindRegistryError> {
    match raw {
        "session" => Ok(MemoryScope::Session),
        "project" => Ok(MemoryScope::Project),
        "global" => Ok(MemoryScope::Global),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown default_scope: {raw}"
        ))),
    }
}

fn parse_inject(raw: &str) -> Result<MemoryInjectPolicy, MemoryKindRegistryError> {
    match raw {
        "pinned" => Ok(MemoryInjectPolicy::Pinned),
        "on_demand" => Ok(MemoryInjectPolicy::OnDemand),
        "manual" => Ok(MemoryInjectPolicy::Manual),
        "never" => Ok(MemoryInjectPolicy::Never),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown default_inject: {raw}"
        ))),
    }
}

fn parse_status(raw: &str) -> Result<MemoryStatus, MemoryKindRegistryError> {
    match raw {
        "active" => Ok(MemoryStatus::Active),
        "inactive" => Ok(MemoryStatus::Inactive),
        "open" => Ok(MemoryStatus::Open),
        "archived" => Ok(MemoryStatus::Archived),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown status: {raw}"
        ))),
    }
}

fn parse_lifecycle(raw: &str) -> Result<MemoryLifecycle, MemoryKindRegistryError> {
    match raw {
        "active_inactive" => Ok(MemoryLifecycle::ActiveInactive),
        "open_archive" => Ok(MemoryLifecycle::OpenArchive),
        "active_archive" => Ok(MemoryLifecycle::ActiveArchive),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown lifecycle: {raw}"
        ))),
    }
}

fn parse_cardinality(raw: &str) -> Result<MemoryCardinality, MemoryKindRegistryError> {
    match raw {
        "single_effective" => Ok(MemoryCardinality::SingleEffective),
        "multiple" => Ok(MemoryCardinality::Multiple),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown cardinality: {raw}"
        ))),
    }
}

fn parse_stale(raw: &str) -> Result<MemoryStalePolicy, MemoryKindRegistryError> {
    match raw {
        "none" => Ok(MemoryStalePolicy::None),
        "session_changed" => Ok(MemoryStalePolicy::SessionChanged),
        _ => Err(MemoryKindRegistryError::Parse(format!(
            "unknown stale: {raw}"
        ))),
    }
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

pub fn baseline_memory_kind_registry() -> &'static MemoryKindRegistry {
    static REGISTRY: OnceLock<MemoryKindRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| MemoryKindRegistry::baseline().expect("baseline AISH memory pack"))
}

/// 後方互換 alias（0039 以前の呼び出し元向け）。
pub fn builtin_memory_kind_registry() -> &'static MemoryKindRegistry {
    baseline_memory_kind_registry()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> MemoryKindRegistry {
        MemoryKindRegistry::baseline().expect("baseline pack")
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
