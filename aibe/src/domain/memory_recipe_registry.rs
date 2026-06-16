//! Memory recipe 定義の正本（TOML pack + markdown prompt）。

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use aibe_protocol::{MemoryQueryDto, MemoryScopeDto, MemoryStatusDto};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct RecipeMaterialQuery {
    pub name: String,
    pub title: String,
    pub query: MemoryQueryDto,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeOutputContract {
    pub format: String,
    pub summary_required: bool,
    pub allow_operations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryRecipeDefinition {
    pub id: String,
    pub description: String,
    pub llm_profile: Option<String>,
    pub system_prompt: String,
    pub materials: Vec<RecipeMaterialQuery>,
    pub output: RecipeOutputContract,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MemoryRecipeRegistryError {
    #[error("recipe registry io: {0}")]
    Io(String),
    #[error("recipe registry parse: {0}")]
    Parse(String),
    #[error("recipe registry unknown recipe: {0}")]
    UnknownRecipe(String),
}

#[derive(Debug, Clone)]
pub struct MemoryRecipeRegistry {
    recipes: HashMap<String, MemoryRecipeDefinition>,
}

impl MemoryRecipeRegistry {
    pub fn empty() -> Self {
        Self {
            recipes: HashMap::new(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&MemoryRecipeDefinition> {
        self.recipes.get(id)
    }

    pub fn merge(&mut self, other: Self) {
        for (id, def) in other.recipes {
            self.recipes.insert(id, def);
        }
    }

    pub fn load_from_str(
        raw: &str,
        source: &str,
        system_prompt: &str,
    ) -> Result<Self, MemoryRecipeRegistryError> {
        let entry = parse_recipe_toml(raw, source, system_prompt)?;
        let mut registry = Self::empty();
        registry.recipes.insert(entry.id.clone(), entry);
        Ok(registry)
    }

    pub fn baseline() -> Result<Self, MemoryRecipeRegistryError> {
        const BASELINE_RECIPE_TOML: &str =
            include_str!("../../memory/packs/aish-memory/recipes/clarify-goal.toml");
        const BASELINE_RECIPE_MD: &str =
            include_str!("../../memory/packs/aish-memory/recipes/clarify-goal.md");
        Self::load_from_str(
            BASELINE_RECIPE_TOML,
            "aish-memory/recipes/clarify-goal.toml",
            BASELINE_RECIPE_MD,
        )
    }
}

pub fn baseline_memory_recipe_registry() -> &'static MemoryRecipeRegistry {
    static REGISTRY: OnceLock<MemoryRecipeRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| MemoryRecipeRegistry::baseline().expect("baseline AISH recipe pack"))
}

#[derive(Debug, Deserialize)]
struct RecipeTomlRoot {
    id: String,
    description: Option<String>,
    llm_profile: Option<String>,
    prompt_md: String,
    #[serde(default)]
    materials: Vec<MaterialTomlEntry>,
    output: OutputTomlEntry,
}

#[derive(Debug, Deserialize)]
struct MaterialTomlEntry {
    name: String,
    title: String,
    kind: String,
    scope: String,
    status: String,
    #[serde(default)]
    active_only: bool,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    optional: bool,
}

#[derive(Debug, Deserialize)]
struct OutputTomlEntry {
    format: String,
    summary_required: bool,
    #[serde(default)]
    allow_operations: Vec<String>,
}

fn parse_recipe_toml(
    raw: &str,
    source: &str,
    system_prompt: &str,
) -> Result<MemoryRecipeDefinition, MemoryRecipeRegistryError> {
    let root: RecipeTomlRoot = toml::from_str(raw)
        .map_err(|e| MemoryRecipeRegistryError::Parse(format!("{source}: {e}")))?;
    let _ = (&root.prompt_md, source);

    let mut seen = HashSet::new();
    let mut materials = Vec::with_capacity(root.materials.len());
    for entry in root.materials {
        if !seen.insert(entry.name.clone()) {
            return Err(MemoryRecipeRegistryError::Parse(format!(
                "{source}: duplicate material name: {}",
                entry.name
            )));
        }
        materials.push(RecipeMaterialQuery {
            name: entry.name,
            title: entry.title,
            query: MemoryQueryDto {
                kind: Some(entry.kind),
                scope: Some(parse_scope(&entry.scope)?),
                status: Some(parse_status(&entry.status)?),
                active_only: entry.active_only,
                include_archived: false,
                limit: entry.limit,
                include_prompt_block: false,
                user_query: None,
            },
            optional: entry.optional,
        });
    }

    Ok(MemoryRecipeDefinition {
        id: root.id,
        description: root.description.unwrap_or_default(),
        llm_profile: root.llm_profile,
        system_prompt: system_prompt.to_string(),
        materials,
        output: RecipeOutputContract {
            format: root.output.format,
            summary_required: root.output.summary_required,
            allow_operations: root.output.allow_operations,
        },
    })
}

fn parse_scope(raw: &str) -> Result<MemoryScopeDto, MemoryRecipeRegistryError> {
    match raw {
        "session" => Ok(MemoryScopeDto::Session),
        "project" => Ok(MemoryScopeDto::Project),
        "global" => Ok(MemoryScopeDto::Global),
        _ => Err(MemoryRecipeRegistryError::Parse(format!(
            "unknown scope: {raw}"
        ))),
    }
}

fn parse_status(raw: &str) -> Result<MemoryStatusDto, MemoryRecipeRegistryError> {
    match raw {
        "active" => Ok(MemoryStatusDto::Active),
        "inactive" => Ok(MemoryStatusDto::Inactive),
        "open" => Ok(MemoryStatusDto::Open),
        "archived" => Ok(MemoryStatusDto::Archived),
        _ => Err(MemoryRecipeRegistryError::Parse(format!(
            "unknown status: {raw}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_clarify_goal_loads() {
        let reg = MemoryRecipeRegistry::baseline().expect("baseline");
        let def = reg.get("clarify-goal").expect("recipe");
        assert_eq!(def.materials.len(), 5);
        assert!(def.system_prompt.contains("JSON"));
        assert_eq!(def.materials[0].name, "open_query");
        assert_eq!(def.materials[0].title, "Open ideas");
        assert_eq!(def.materials[1].name, "goal_query");
        assert_eq!(def.materials[1].title, "Active goal");
    }

    #[test]
    fn materials_preserve_toml_order_and_titles() {
        let raw = r#"
id = "ordered"
description = "test"
llm_profile = "default"
prompt_md = "x.md"

[[materials]]
name = "first"
title = "First section"
kind = "note"
scope = "project"
status = "open"

[[materials]]
name = "second"
title = "Second section"
kind = "rule"
scope = "project"
status = "active"

[output]
format = "json"
summary_required = true
allow_operations = ["add"]
"#;
        let def = parse_recipe_toml(raw, "test.toml", "# prompt").expect("parse");
        assert_eq!(def.materials.len(), 2);
        assert_eq!(def.materials[0].name, "first");
        assert_eq!(def.materials[0].title, "First section");
        assert_eq!(def.materials[1].name, "second");
        assert_eq!(def.materials[1].title, "Second section");
    }

    #[test]
    fn duplicate_material_name_is_rejected() {
        let raw = r#"
id = "dup"
description = "test"
llm_profile = "default"
prompt_md = "x.md"

[[materials]]
name = "same"
title = "A"
kind = "note"
scope = "project"
status = "open"

[[materials]]
name = "same"
title = "B"
kind = "rule"
scope = "project"
status = "active"

[output]
format = "json"
summary_required = true
allow_operations = ["add"]
"#;
        let err = parse_recipe_toml(raw, "dup.toml", "# prompt").unwrap_err();
        assert!(
            matches!(err, MemoryRecipeRegistryError::Parse(ref m) if m.contains("duplicate material name"))
        );
    }
}
