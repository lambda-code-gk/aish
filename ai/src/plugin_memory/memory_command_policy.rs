//! `memory_kind_list` snapshot から CLI policy を導く。

use aibe_protocol::{
    MemoryKindDefinitionDto, MemoryOperationAdd, MemoryOperationDto, MemoryScopeDto,
    MemoryStatusDto,
};

/// command 単位で 1 回だけ取得した kind registry snapshot。
#[derive(Debug, Clone)]
pub struct MemoryCommandPolicy {
    kinds: Vec<MemoryKindDefinitionDto>,
}

impl MemoryCommandPolicy {
    pub fn from_kinds(kinds: Vec<MemoryKindDefinitionDto>) -> Self {
        Self { kinds }
    }

    pub fn kinds(&self) -> &[MemoryKindDefinitionDto] {
        &self.kinds
    }

    pub fn ordered_kinds(&self) -> Vec<MemoryKindDefinitionDto> {
        let mut kinds = self.kinds.clone();
        kinds.sort_by_key(|k| (k.priority, k.id.clone()));
        kinds
    }

    pub fn find_kind(&self, kind: &str) -> Option<&MemoryKindDefinitionDto> {
        self.kinds.iter().find(|k| k.id == kind)
    }

    /// `mem add <kind>` が専用 CLI 向け kind なら誘導メッセージを返す。
    pub fn mem_add_dedicated_hint(&self, kind: &str) -> Option<String> {
        let def = self.find_kind(kind)?;
        def.dedicated_cli
            .as_ref()
            .map(|cli| format!("{kind} is a standard memory kind; use `{cli} ...`"))
    }

    pub fn clear_scope(&self, kind: &str) -> MemoryScopeDto {
        self.find_kind(kind)
            .map(|d| d.default_scope)
            .unwrap_or(MemoryScopeDto::Project)
    }

    pub fn add_operation(&self, kind: &str, text: &str) -> MemoryOperationDto {
        if let Some(def) = self.find_kind(kind) {
            MemoryOperationDto::Add(MemoryOperationAdd {
                kind: kind.to_string(),
                scope: Some(def.default_scope),
                inject: Some(def.default_inject),
                status: Some(def.default_status),
                text: text.to_string(),
                make_active: Some(def.default_status == MemoryStatusDto::Active),
            })
        } else {
            MemoryOperationDto::Add(MemoryOperationAdd {
                kind: kind.to_string(),
                text: text.to_string(),
                scope: None,
                inject: None,
                status: None,
                make_active: None,
            })
        }
    }

    pub fn generic_add_operation(&self, kind: &str, text: &str) -> MemoryOperationDto {
        MemoryOperationDto::Add(MemoryOperationAdd {
            kind: kind.to_string(),
            text: text.to_string(),
            scope: None,
            inject: None,
            status: None,
            make_active: None,
        })
    }

    /// 専用 CLI の show（active の 1 件）。
    pub fn show_query_status(&self, kind: &str) -> MemoryStatusDto {
        self.find_kind(kind)
            .map(|_| MemoryStatusDto::Active)
            .unwrap_or(MemoryStatusDto::Active)
    }

    pub fn show_query_scope(&self, kind: &str) -> MemoryScopeDto {
        self.clear_scope(kind)
    }

    /// 専用 CLI の list（既定 status で列挙）。
    pub fn list_query_status(&self, kind: &str) -> MemoryStatusDto {
        self.find_kind(kind)
            .map(|d| d.default_status)
            .unwrap_or(MemoryStatusDto::Open)
    }

    pub fn list_query_scope(&self, kind: &str) -> MemoryScopeDto {
        self.clear_scope(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::MemoryInjectPolicyDto;

    fn kind_with_dedicated_cli(
        id: &str,
        cli: &str,
        scope: MemoryScopeDto,
    ) -> MemoryKindDefinitionDto {
        MemoryKindDefinitionDto {
            id: id.to_string(),
            description: format!("{id} desc"),
            default_scope: scope,
            default_inject: MemoryInjectPolicyDto::Pinned,
            default_status: MemoryStatusDto::Active,
            lifecycle: "active_inactive".into(),
            cardinality: "single_effective".into(),
            clear_from: MemoryStatusDto::Active,
            clear_to: MemoryStatusDto::Inactive,
            auto_inject: true,
            on_demand: false,
            priority: 10,
            keywords: vec![],
            max_entries: Some(1),
            aliases: vec![id.to_string()],
            builtin: true,
            dedicated_cli: Some(cli.to_string()),
        }
    }

    fn idea_kind() -> MemoryKindDefinitionDto {
        MemoryKindDefinitionDto {
            id: "idea".into(),
            description: "ideas".into(),
            default_scope: MemoryScopeDto::Project,
            default_inject: MemoryInjectPolicyDto::OnDemand,
            default_status: MemoryStatusDto::Open,
            lifecycle: "open_archive".into(),
            cardinality: "multiple".into(),
            clear_from: MemoryStatusDto::Open,
            clear_to: MemoryStatusDto::Archived,
            auto_inject: false,
            on_demand: true,
            priority: 80,
            keywords: vec![],
            max_entries: Some(12),
            aliases: vec!["idea".into()],
            builtin: true,
            dedicated_cli: Some("ai idea add".into()),
        }
    }

    #[test]
    fn mem_add_hint_from_dedicated_cli_metadata() {
        let policy = MemoryCommandPolicy::from_kinds(vec![kind_with_dedicated_cli(
            "goal",
            "ai goal set",
            MemoryScopeDto::Project,
        )]);
        let hint = policy.mem_add_dedicated_hint("goal").expect("hint");
        assert!(hint.contains("ai goal set"));
        assert!(policy.mem_add_dedicated_hint("rule").is_none());
    }

    #[test]
    fn clear_scope_uses_default_scope_from_metadata() {
        let policy = MemoryCommandPolicy::from_kinds(vec![kind_with_dedicated_cli(
            "now",
            "ai now set",
            MemoryScopeDto::Session,
        )]);
        assert_eq!(policy.clear_scope("now"), MemoryScopeDto::Session);
        assert_eq!(policy.clear_scope("unknown"), MemoryScopeDto::Project);
    }

    #[test]
    fn add_operation_uses_metadata_defaults() {
        let policy = MemoryCommandPolicy::from_kinds(vec![idea_kind()]);
        match policy.add_operation("idea", "memo") {
            MemoryOperationDto::Add(add) => {
                assert_eq!(add.kind, "idea");
                assert_eq!(add.scope, Some(MemoryScopeDto::Project));
                assert_eq!(add.inject, Some(MemoryInjectPolicyDto::OnDemand));
                assert_eq!(add.status, Some(MemoryStatusDto::Open));
                assert_eq!(add.make_active, Some(false));
            }
            _ => panic!("expected add"),
        }
    }

    #[test]
    fn list_query_status_follows_default_status() {
        let policy = MemoryCommandPolicy::from_kinds(vec![idea_kind()]);
        assert_eq!(policy.list_query_status("idea"), MemoryStatusDto::Open);
        assert_eq!(policy.show_query_status("goal"), MemoryStatusDto::Active);
    }

    #[test]
    fn ordered_kinds_sorts_only_for_display() {
        let policy = MemoryCommandPolicy::from_kinds(vec![
            kind_with_dedicated_cli("note", "ai mem add note", MemoryScopeDto::Project),
            kind_with_dedicated_cli("goal", "ai goal set", MemoryScopeDto::Project),
        ]);
        let raw_ids = policy
            .kinds()
            .iter()
            .map(|kind| kind.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(raw_ids, vec!["note", "goal"]);

        let ordered = policy.ordered_kinds();
        let ordered_ids = ordered
            .iter()
            .map(|kind| kind.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ordered_ids, vec!["goal", "note"]);
    }
}
