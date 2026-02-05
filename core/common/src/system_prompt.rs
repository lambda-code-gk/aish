//! システムプロンプト（sysq）のドメインとマージロジック
//!
//! グローバル・ユーザー・プロジェクトの有効IDを優先順位でマージする純粋関数を提供する。

use std::collections::HashMap;
use std::path::PathBuf;

/// システムプロンプトのスコープ（優先度: Project > User > Global）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    User,
    Project,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::User => "user",
            Scope::Project => "project",
        }
    }
}

/// 各スコープの「利用可能なID→パス」と「有効IDの並び」から、
/// 優先順位を適用した最終的な (id, 採用するパス) の順序付きリストを返す。
///
/// 同じIDが複数スコープにあれば、Project > User > Global でパスを上書きする。
/// 並び順は global → user → project の有効IDをその順で重複なし。
pub fn merge_enabled_ordered(
    global_available: &HashMap<String, PathBuf>,
    global_enabled: &[String],
    user_available: &HashMap<String, PathBuf>,
    user_enabled: &[String],
    project_available: &HashMap<String, PathBuf>,
    project_enabled: &[String],
) -> Vec<(String, PathBuf)> {
    use std::collections::HashSet;
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut result_paths: HashMap<String, PathBuf> = HashMap::new();

    for id in global_enabled {
        if global_available.contains_key(id) && !seen.contains(id.as_str()) {
            seen.insert(id.as_str());
            order.push(id.clone());
        }
    }
    for id in user_enabled {
        if user_available.contains_key(id) && !seen.contains(id.as_str()) {
            seen.insert(id.as_str());
            order.push(id.clone());
        }
    }
    for id in project_enabled {
        if project_available.contains_key(id) && !seen.contains(id.as_str()) {
            seen.insert(id.as_str());
            order.push(id.clone());
        }
    }

    for id in global_enabled {
        if let Some(p) = global_available.get(id) {
            result_paths.insert(id.clone(), p.clone());
        }
    }
    for id in user_enabled {
        if let Some(p) = user_available.get(id) {
            result_paths.insert(id.clone(), p.clone());
        }
    }
    for id in project_enabled {
        if let Some(p) = project_available.get(id) {
            result_paths.insert(id.clone(), p.clone());
        }
    }

    order
        .into_iter()
        .filter_map(|id| result_paths.remove(&id).map(|path| (id, path)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_global_only() {
        let mut g = HashMap::new();
        g.insert("a".to_string(), PathBuf::from("/g/a"));
        let r = merge_enabled_ordered(&g, &["a".to_string()], &HashMap::new(), &[], &HashMap::new(), &[]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "a");
        assert_eq!(r[0].1, PathBuf::from("/g/a"));
    }

    #[test]
    fn test_merge_project_overrides_global() {
        let mut g = HashMap::new();
        g.insert("a".to_string(), PathBuf::from("/g/a"));
        let mut p = HashMap::new();
        p.insert("a".to_string(), PathBuf::from("/p/a"));
        let r = merge_enabled_ordered(
            &g,
            &["a".to_string()],
            &HashMap::new(),
            &[],
            &p,
            &["a".to_string()],
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "a");
        assert_eq!(r[0].1, PathBuf::from("/p/a"));
    }

    #[test]
    fn test_merge_order_preserved() {
        let mut g = HashMap::new();
        g.insert("a".to_string(), PathBuf::from("/g/a"));
        g.insert("b".to_string(), PathBuf::from("/g/b"));
        let mut u = HashMap::new();
        u.insert("c".to_string(), PathBuf::from("/u/c"));
        let r = merge_enabled_ordered(
            &g,
            &["a".to_string(), "b".to_string()],
            &u,
            &["c".to_string()],
            &HashMap::new(),
            &[],
        );
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].0, "a");
        assert_eq!(r[1].0, "b");
        assert_eq!(r[2].0, "c");
    }
}
