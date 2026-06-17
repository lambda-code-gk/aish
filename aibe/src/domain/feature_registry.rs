//! Smart feature 定義の registry（0042）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use aibe_protocol::FeatureAction;
use serde::Deserialize;

/// feature registry の解決モード（0043 Phase 3）。composition root が決定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveFeatureMode {
    /// 空 registry（明示空または generic memory 時の `feature_files=None`）。
    Empty,
    /// baseline pack 互換（`feature_files=None`、generic memory ではない）。
    BaselineCompat,
    /// `feature_files` に列挙された TOML のみを merge。
    ExplicitFiles,
}

/// feature pack の入力（memory pack から独立した設定面、0043 Phase 3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePackConfig {
    pub feature_files: Vec<PathBuf>,
}

impl FeaturePackConfig {
    pub fn empty() -> Self {
        Self {
            feature_files: Vec::new(),
        }
    }

    pub fn explicit_files(files: Vec<PathBuf>) -> Self {
        Self {
            feature_files: files,
        }
    }
}

/// feature pack の解決結果（composition root が一度だけ決める）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePackResolution {
    pub mode: EffectiveFeatureMode,
    pub config: FeaturePackConfig,
}

impl FeaturePackResolution {
    pub fn empty() -> Self {
        Self {
            mode: EffectiveFeatureMode::Empty,
            config: FeaturePackConfig::empty(),
        }
    }

    pub fn baseline_compat() -> Self {
        Self {
            mode: EffectiveFeatureMode::BaselineCompat,
            config: FeaturePackConfig::empty(),
        }
    }

    pub fn explicit_files(files: Vec<PathBuf>) -> Self {
        Self {
            mode: EffectiveFeatureMode::ExplicitFiles,
            config: FeaturePackConfig::explicit_files(files),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FeatureRegistryError {
    #[error("failed to parse features: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(String),
}

#[derive(Debug, Clone)]
pub struct FeatureDefinition {
    pub id: String,
    pub description: Option<String>,
    pub triggers: Vec<String>,
    pub actions: Vec<FeatureAction>,
    pub priority: u32,
    pub requires_memory: bool,
    pub requires_recipe: bool,
}

/// trigger 一致後の eligibility 判定用（0043 Phase 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureEligibilityContext {
    pub memory_kinds_enabled: bool,
    pub recipes_enabled: bool,
}

impl Default for FeatureEligibilityContext {
    fn default() -> Self {
        Self {
            memory_kinds_enabled: true,
            recipes_enabled: true,
        }
    }
}

impl FeatureEligibilityContext {
    pub fn from_memory_kinds_and_recipes(
        memory_kinds_enabled: bool,
        recipes_enabled: bool,
    ) -> Self {
        Self {
            memory_kinds_enabled,
            recipes_enabled,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FeatureRegistry {
    features: HashMap<String, FeatureDefinition>,
    /// TOML 定義順（merge 時は後勝ちで上書き、新規 id のみ末尾追加）。
    feature_order: Vec<String>,
}

impl FeatureRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn baseline_pack_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("memory/packs/aish-memory/features.toml")
    }

    pub fn baseline() -> Result<Self, FeatureRegistryError> {
        const BASELINE: &str = include_str!("../../memory/packs/aish-memory/features.toml");
        Self::load_from_str(BASELINE, "aish-memory/features.toml")
    }

    pub fn load_from_str(raw: &str, source: &str) -> Result<Self, FeatureRegistryError> {
        let table: toml::Table = toml::from_str(raw)
            .map_err(|e| FeatureRegistryError::Parse(format!("{source}: {e}")))?;
        let mut features = HashMap::new();
        let mut feature_order = Vec::new();
        for (id, value) in table {
            let toml::Value::Table(section) = value else {
                continue;
            };
            let def = parse_feature_definition(&id, section, source)?;
            features.insert(id.clone(), def);
            feature_order.push(id);
        }
        Ok(Self {
            features,
            feature_order,
        })
    }

    pub fn merge(&mut self, other: Self) {
        for id in other.feature_order {
            if let Some(def) = other.features.get(&id) {
                if self.features.insert(id.clone(), def.clone()).is_none() {
                    self.feature_order.push(id);
                }
            }
        }
    }

    pub fn feature_ids(&self) -> Vec<&str> {
        self.feature_order
            .iter()
            .filter(|id| self.features.contains_key(*id))
            .map(String::as_str)
            .collect()
    }

    pub fn catalog_for_prompt(&self) -> String {
        let ids = self.feature_ids();
        if ids.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "Configured smart features (reference; still emit feature_actions JSON):\n",
        );
        for id in ids {
            let Some(def) = self.features.get(id) else {
                continue;
            };
            let desc = def.description.as_deref().unwrap_or("(no description)");
            out.push_str(&format!("- {id}: {desc}\n"));
        }
        out
    }

    /// user query に trigger が部分一致する feature の actions を返す（順序は定義順）。
    pub fn match_query(&self, query: &str) -> Vec<FeatureAction> {
        self.match_eligible_actions(query, FeatureEligibilityContext::default())
    }

    /// trigger 候補を eligibility で落とし、priority 昇順で actions を返す。
    pub fn match_eligible_actions(
        &self,
        query: &str,
        ctx: FeatureEligibilityContext,
    ) -> Vec<FeatureAction> {
        let q = query.to_ascii_lowercase();
        let mut matched: Vec<&FeatureDefinition> = Vec::new();
        for id in &self.feature_order {
            let Some(def) = self.features.get(id) else {
                continue;
            };
            if !def
                .triggers
                .iter()
                .any(|t| query_contains_trigger(&q, query, t))
            {
                continue;
            }
            if def.requires_memory && !ctx.memory_kinds_enabled {
                continue;
            }
            if def.requires_recipe && !ctx.recipes_enabled {
                continue;
            }
            matched.push(def);
        }
        matched.sort_by_key(|def| def.priority);
        let mut out = Vec::new();
        for def in matched {
            for action in &def.actions {
                if !out
                    .iter()
                    .any(|existing| actions_equivalent(existing, action))
                {
                    out.push(action.clone());
                }
            }
        }
        out
    }
}

pub fn baseline_feature_registry() -> &'static FeatureRegistry {
    static REGISTRY: OnceLock<FeatureRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| FeatureRegistry::baseline().expect("baseline AISH features pack"))
}

#[derive(Debug, Deserialize)]
struct FeatureDefinitionToml {
    description: Option<String>,
    #[serde(default)]
    triggers: Vec<String>,
    #[serde(default)]
    actions: Vec<toml::Value>,
    #[serde(default = "default_feature_priority")]
    priority: u32,
    #[serde(default)]
    requires_memory: bool,
    #[serde(default)]
    requires_recipe: bool,
}

fn default_feature_priority() -> u32 {
    100
}

fn parse_feature_definition(
    id: &str,
    section: toml::map::Map<String, toml::Value>,
    source: &str,
) -> Result<FeatureDefinition, FeatureRegistryError> {
    let value = toml::Value::Table(section);
    let parsed: FeatureDefinitionToml = value
        .try_into()
        .map_err(|e| FeatureRegistryError::Parse(format!("{source} feature '{id}': {e}")))?;
    let mut actions = Vec::new();
    for (idx, action_value) in parsed.actions.into_iter().enumerate() {
        let json = toml_value_to_json(action_value);
        match serde_json::from_value::<FeatureAction>(json) {
            Ok(FeatureAction::Unsupported) => {}
            Ok(FeatureAction::MemoryRecipeRun { apply: true, .. }) => {}
            Ok(action) => actions.push(action),
            Err(e) => {
                return Err(FeatureRegistryError::Parse(format!(
                    "{source} feature '{id}' action[{idx}]: {e}"
                )));
            }
        }
    }
    Ok(FeatureDefinition {
        id: id.to_string(),
        description: parsed.description,
        triggers: parsed.triggers,
        actions,
        priority: parsed.priority,
        requires_memory: parsed.requires_memory,
        requires_recipe: parsed.requires_recipe,
    })
}

fn query_contains_trigger(q_lower: &str, query_original: &str, trigger: &str) -> bool {
    let t = trigger.trim();
    if t.is_empty() {
        return false;
    }
    if query_original.contains(t) {
        return true;
    }
    q_lower.contains(&t.to_ascii_lowercase())
}

fn toml_value_to_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

pub fn actions_equivalent(a: &FeatureAction, b: &FeatureAction) -> bool {
    match (a, b) {
        (FeatureAction::MemoryQuery { query: qa }, FeatureAction::MemoryQuery { query: qb }) => {
            memory_query_equivalent(qa, qb)
        }
        (
            FeatureAction::MemoryRecipeRun {
                recipe_id: ra,
                apply: aa,
            },
            FeatureAction::MemoryRecipeRun {
                recipe_id: rb,
                apply: ab,
            },
        ) => ra == rb && aa == ab,
        (
            FeatureAction::SetLogTailBytes { bytes: ba },
            FeatureAction::SetLogTailBytes { bytes: bb },
        ) => ba == bb,
        (
            FeatureAction::SetRecommendedTools { tools: ta },
            FeatureAction::SetRecommendedTools { tools: tb },
        ) => ta == tb,
        _ => false,
    }
}

fn memory_query_equivalent(
    a: &aibe_protocol::MemoryQueryDto,
    b: &aibe_protocol::MemoryQueryDto,
) -> bool {
    let mut left = a.clone();
    let mut right = b.clone();
    // user_query は executor が user input から補完するため、重複判定から除外する。
    left.user_query = None;
    right.user_query = None;
    serde_json::to_value(&left).ok() == serde_json::to_value(&right).ok()
}

pub fn feature_action_schema_prompt() -> &'static str {
    r#"Allowed feature_actions (MVP). Return an array; use [] when none apply. Do not invent action types.

1. memory_query — read contextual memory (goal, rule, decision, etc.)
   {"type":"memory_query","query":{"include_prompt_block":true,"user_query":"..."}}
   Use when the user asks about current goal, project rules, decisions, or prior context.

2. memory_recipe_run — propose memory updates without applying (apply MUST be false)
   {"type":"memory_recipe_run","recipe_id":"clarify-goal","apply":false}
   Use when the user wants to clarify goals, organize work, or decide next actions.

3. set_log_tail_bytes — include more shell log in context (read-only)
   {"type":"set_log_tail_bytes","bytes":20480}
   Use when the user asks about recent shell errors, command output, failures, or logs.

4. set_recommended_tools — suggest read-only inspection tools (never include shell_exec)
   {"type":"set_recommended_tools","tools":["read_file","grep","git_status"]}
   Use when repository inspection is needed.

Never return apply=true for memory_recipe_run. Never include shell_exec in set_recommended_tools."#
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::{FeatureAction, MemoryQueryDto};

    #[test]
    fn baseline_features_load() {
        let reg = FeatureRegistry::baseline().expect("baseline");
        assert!(reg.features.contains_key("inspect_error"));
        assert!(reg.features.contains_key("clarify_goal"));
    }

    #[test]
    fn match_query_inspect_error_trigger() {
        let reg = FeatureRegistry::baseline().expect("baseline");
        let actions = reg.match_query("直近のエラーを調べて");
        assert!(actions
            .iter()
            .any(|a| matches!(a, FeatureAction::SetLogTailBytes { .. })));
        assert!(actions
            .iter()
            .any(|a| matches!(a, FeatureAction::SetRecommendedTools { .. })));
    }

    #[test]
    fn match_query_clarify_goal_trigger() {
        let reg = FeatureRegistry::baseline().expect("baseline");
        let actions = reg.match_query("作業の目的を整理したい");
        assert!(actions.iter().any(|a| matches!(
            a,
            FeatureAction::MemoryRecipeRun { recipe_id, apply: false }
            if recipe_id == "clarify-goal"
        )));
    }

    #[test]
    fn memory_query_equivalent_ignores_user_query() {
        let with_query = FeatureAction::MemoryQuery {
            query: MemoryQueryDto {
                include_prompt_block: true,
                user_query: Some("プロジェクトのルールは？".into()),
                ..MemoryQueryDto::default()
            },
        };
        let without_query = FeatureAction::MemoryQuery {
            query: MemoryQueryDto {
                include_prompt_block: true,
                ..MemoryQueryDto::default()
            },
        };
        assert!(actions_equivalent(&with_query, &without_query));
    }

    #[test]
    fn match_eligible_actions_skips_requires_memory_when_kinds_disabled() {
        let reg = FeatureRegistry::baseline().expect("baseline");
        let ctx = FeatureEligibilityContext {
            memory_kinds_enabled: false,
            recipes_enabled: true,
        };
        let actions = reg.match_eligible_actions("プロジェクトのルールは？", ctx);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, FeatureAction::MemoryQuery { .. })));
    }

    #[test]
    fn match_eligible_actions_skips_requires_recipe_when_recipes_disabled() {
        let reg = FeatureRegistry::baseline().expect("baseline");
        let ctx = FeatureEligibilityContext {
            memory_kinds_enabled: true,
            recipes_enabled: false,
        };
        let actions = reg.match_eligible_actions("作業の目的を整理したい", ctx);
        assert!(!actions
            .iter()
            .any(|a| matches!(a, FeatureAction::MemoryRecipeRun { .. })));
    }

    #[test]
    fn match_eligible_actions_keeps_inspect_error_without_memory_or_recipe() {
        let reg = FeatureRegistry::baseline().expect("baseline");
        let ctx = FeatureEligibilityContext {
            memory_kinds_enabled: false,
            recipes_enabled: false,
        };
        let actions = reg.match_eligible_actions("直近のエラーを調べて", ctx);
        assert!(actions
            .iter()
            .any(|a| matches!(a, FeatureAction::SetLogTailBytes { .. })));
    }
}
