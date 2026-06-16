//! MemoryRecipe domain（材料収集・LLM 出力検証・prompt 生成）。

use aibe_protocol::MemoryOperationDto;

use super::contextual_memory::{resolve_memory_operation_add, MemoryEntry, MemoryValidationError};
use super::memory_kind_registry::MemoryKindRegistry;
use super::memory_recipe_registry::MemoryRecipeDefinition;
use crate::ports::outbound::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemoryStoreContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeMaterials {
    pub sections: Vec<(String, RecipeMaterialValue)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeMaterialValue {
    Many(Vec<MemoryEntry>),
    One(Option<MemoryEntry>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRecipeProposal {
    pub operation: MemoryOperationDto,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRecipeOutput {
    pub summary: String,
    pub proposals: Vec<ValidatedRecipeProposal>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MemoryRecipeError {
    #[error("unknown recipe: {0}")]
    UnknownRecipe(String),
    #[error("recipe llm output invalid: {0}")]
    InvalidLlmOutput(String),
    #[error("recipe proposal invalid: {0}")]
    InvalidProposal(String),
    #[error("store: {0}")]
    Store(String),
}

impl From<ContextualMemoryStoreError> for MemoryRecipeError {
    fn from(err: ContextualMemoryStoreError) -> Self {
        Self::Store(err.to_string())
    }
}

impl From<MemoryValidationError> for MemoryRecipeError {
    fn from(err: MemoryValidationError) -> Self {
        Self::InvalidProposal(err.to_string())
    }
}

/// recipe 定義に従い材料 memory を store から収集する。
pub fn collect_recipe_materials(
    store: &dyn ContextualMemoryStore,
    ctx: &MemoryStoreContext<'_>,
    recipe: &MemoryRecipeDefinition,
) -> Result<RecipeMaterials, MemoryRecipeError> {
    let mut sections = Vec::with_capacity(recipe.materials.len());
    for material in &recipe.materials {
        let entries = store.query(ctx, &material.query)?;
        let value = if material.query.limit == Some(1) {
            RecipeMaterialValue::One(entries.into_iter().next())
        } else {
            RecipeMaterialValue::Many(entries)
        };
        sections.push((material.name.clone(), value));
    }
    Ok(RecipeMaterials { sections })
}

/// recipe 定義と材料から LLM messages（system + user）を組み立てる。
pub fn build_recipe_messages(
    recipe: &MemoryRecipeDefinition,
    materials: &RecipeMaterials,
    user_instruction: Option<&str>,
) -> (String, String) {
    let system = recipe.system_prompt.clone();
    let mut user = String::new();
    for material in &recipe.materials {
        let title = &material.title;
        let section = materials
            .sections
            .iter()
            .find_map(|(name, value)| (name == &material.name).then_some(value));
        match section {
            Some(RecipeMaterialValue::Many(entries)) => {
                user.push_str(&format_material_section(title, entries));
            }
            Some(RecipeMaterialValue::One(entry)) => {
                user.push_str(&format_material_optional(title, entry.as_ref()));
            }
            None => {
                user.push_str(&format_material_section(title, &[]));
            }
        }
    }
    if let Some(extra) = user_instruction.filter(|s| !s.trim().is_empty()) {
        user.push_str("\nUser instruction:\n");
        user.push_str(extra.trim());
        user.push('\n');
    }
    (system, user)
}

fn format_material_section(title: &str, entries: &[MemoryEntry]) -> String {
    let mut out = format!("{title}:\n");
    if entries.is_empty() {
        out.push_str("  (none)\n");
        return out;
    }
    for entry in entries {
        out.push_str(&format!("  - [{}] {}\n", entry.kind, entry.text));
    }
    out
}

fn format_material_optional(title: &str, entry: Option<&MemoryEntry>) -> String {
    match entry {
        Some(e) => format!("{title}:\n  - [{}] {}\n", e.kind, e.text),
        None => format!("{title}:\n  (none)\n"),
    }
}

/// LLM 生出力を JSON として検証する（markdown fence 不可）。
pub fn parse_and_validate_recipe_output(
    raw: &str,
    registry: &MemoryKindRegistry,
    allow_operations: &[String],
) -> Result<ValidatedRecipeOutput, MemoryRecipeError> {
    let trimmed = raw.trim();
    if trimmed.contains("```") {
        return Err(MemoryRecipeError::InvalidLlmOutput(
            "markdown fences are not allowed".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| MemoryRecipeError::InvalidLlmOutput(format!("invalid json: {e}")))?;
    if !value.is_object() {
        return Err(MemoryRecipeError::InvalidLlmOutput(
            "expected a single json object".into(),
        ));
    }
    if let Some(obj) = value.as_object() {
        for key in obj.keys() {
            if key != "summary" && key != "proposals" {
                return Err(MemoryRecipeError::InvalidLlmOutput(format!(
                    "unknown field: {key}"
                )));
            }
        }
    }

    let summary = value
        .get("summary")
        .and_then(|v| v.as_str())
        .ok_or_else(|| MemoryRecipeError::InvalidLlmOutput("missing summary".into()))?
        .trim()
        .to_string();
    if summary.is_empty() {
        return Err(MemoryRecipeError::InvalidLlmOutput(
            "summary must not be empty".into(),
        ));
    }

    let proposals_value = value
        .get("proposals")
        .ok_or_else(|| MemoryRecipeError::InvalidLlmOutput("missing proposals".into()))?;
    let proposals_array = proposals_value
        .as_array()
        .ok_or_else(|| MemoryRecipeError::InvalidLlmOutput("proposals must be an array".into()))?;

    let mut proposals = Vec::with_capacity(proposals_array.len());
    for (idx, item) in proposals_array.iter().enumerate() {
        proposals.push(parse_proposal_item(item, idx, registry, allow_operations)?);
    }

    Ok(ValidatedRecipeOutput { summary, proposals })
}

fn parse_proposal_item(
    item: &serde_json::Value,
    idx: usize,
    registry: &MemoryKindRegistry,
    allow_operations: &[String],
) -> Result<ValidatedRecipeProposal, MemoryRecipeError> {
    let obj = item.as_object().ok_or_else(|| {
        MemoryRecipeError::InvalidProposal(format!("proposals[{idx}] must be an object"))
    })?;
    for key in obj.keys() {
        if key != "operation" && key != "rationale" {
            return Err(MemoryRecipeError::InvalidProposal(format!(
                "proposals[{idx}] unknown field: {key}"
            )));
        }
    }
    let operation_value = obj.get("operation").ok_or_else(|| {
        MemoryRecipeError::InvalidProposal(format!("proposals[{idx}] missing operation"))
    })?;
    let operation: MemoryOperationDto =
        serde_json::from_value(operation_value.clone()).map_err(|e| {
            MemoryRecipeError::InvalidProposal(format!("proposals[{idx}] operation: {e}"))
        })?;
    match &operation {
        MemoryOperationDto::Add(add) => {
            if !allow_operations.iter().any(|op| op == "add") {
                return Err(MemoryRecipeError::InvalidProposal(format!(
                    "proposals[{idx}] add operation is not allowed"
                )));
            }
            let _ = registry.get(&add.kind).ok_or_else(|| {
                MemoryRecipeError::InvalidProposal(format!(
                    "proposals[{idx}] unknown kind: {}",
                    add.kind
                ))
            })?;
            let resolved = resolve_memory_operation_add(add, registry)?;
            let operation = MemoryOperationDto::Add(resolved);
            let rationale = obj
                .get("rationale")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    MemoryRecipeError::InvalidProposal(format!(
                        "proposals[{idx}] missing rationale"
                    ))
                })?
                .to_string();
            Ok(ValidatedRecipeProposal {
                operation,
                rationale,
            })
        }
        _ => Err(MemoryRecipeError::InvalidProposal(format!(
            "proposals[{idx}] only {:?} operations are allowed",
            allow_operations
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::MemoryOperationAdd;

    fn registry() -> &'static MemoryKindRegistry {
        super::super::baseline_memory_kind_registry()
    }

    #[test]
    fn parse_valid_recipe_output() {
        let raw = r#"{"summary":"Consolidate ideas","proposals":[{"operation":{"op":"add","kind":"goal","text":"ship v1"},"rationale":"main theme"}]}"#;
        let out =
            parse_and_validate_recipe_output(raw, registry(), &["add".into()]).expect("parse");
        assert_eq!(out.summary, "Consolidate ideas");
        assert_eq!(out.proposals.len(), 1);
        assert_eq!(out.proposals[0].rationale, "main theme");
        match &out.proposals[0].operation {
            MemoryOperationDto::Add(add) => assert_eq!(add.text, "ship v1"),
            _ => panic!("expected add"),
        }
    }

    #[test]
    fn parse_rejects_markdown_fence() {
        let raw = "```json\n{\"summary\":\"x\",\"proposals\":[]}\n```";
        let err = parse_and_validate_recipe_output(raw, registry(), &["add".into()]).unwrap_err();
        assert!(matches!(err, MemoryRecipeError::InvalidLlmOutput(_)));
    }

    #[test]
    fn parse_rejects_unknown_top_level_field() {
        let raw = r#"{"summary":"x","proposals":[],"extra":1}"#;
        let err = parse_and_validate_recipe_output(raw, registry(), &["add".into()]).unwrap_err();
        assert!(matches!(err, MemoryRecipeError::InvalidLlmOutput(_)));
    }

    #[test]
    fn parse_rejects_non_add_operation() {
        let raw = r#"{"summary":"x","proposals":[{"operation":{"op":"clear_kind","kind":"goal","scope":"project"},"rationale":"n"}]}"#;
        let err = parse_and_validate_recipe_output(raw, registry(), &["add".into()]).unwrap_err();
        assert!(matches!(err, MemoryRecipeError::InvalidProposal(_)));
    }

    #[test]
    fn parse_rejects_invalid_kind() {
        let raw = r#"{"summary":"x","proposals":[{"operation":{"op":"add","kind":"not_a_kind","text":"t"},"rationale":"n"}]}"#;
        let err = parse_and_validate_recipe_output(raw, registry(), &["add".into()]).unwrap_err();
        assert!(matches!(err, MemoryRecipeError::InvalidProposal(_)));
    }

    #[test]
    fn build_messages_includes_materials() {
        let recipe = super::super::memory_recipe_registry::MemoryRecipeRegistry::baseline()
            .expect("baseline")
            .get("clarify-goal")
            .expect("recipe")
            .clone();
        let sections = vec![(
            "open_query".into(),
            RecipeMaterialValue::Many(vec![MemoryEntry {
                id: "i1".into(),
                memory_space_id: "ms".into(),
                created_session_id: "s".into(),
                last_session_id: "s".into(),
                kind: "idea".into(),
                scope: super::super::MemoryScope::Project,
                inject: super::super::MemoryInjectPolicy::OnDemand,
                status: super::super::MemoryStatus::Open,
                text: "card idea".into(),
                project_key: None,
                created_at_ms: 1,
                updated_at_ms: 1,
                version: 1,
            }]),
        )];
        let materials = RecipeMaterials { sections };
        let (_system, user) = build_recipe_messages(&recipe, &materials, Some("focus MVP"));
        assert!(user.contains("card idea"));
        assert!(user.contains("Open ideas"));
        assert!(user.contains("focus MVP"));
    }
}
