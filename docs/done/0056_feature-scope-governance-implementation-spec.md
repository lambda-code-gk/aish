# 0056 Feature Scope Governance / Complexity Gate 実装指示書

設計書: [`docs/spec/0056_feature-scope-governance-spec.md`](../spec/0056_feature-scope-governance-spec.md)

## 0. 目的

機能仕様の実装途中膨張を防ぐため、複雑性棚卸し・Scope Lock・CI 検査をリポジトリ標準フローへ導入する。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: `1`
- Complexity class: `governance`（Red、承認例外あり）
- Vertical slice AC ID: `scope_checker_accepts_valid_green_spec`
- Locked AC IDs: 設計書 §9 の 11 件すべて

## 1. Phase 分割

| Phase | 内容 | ゲート |
|-------|------|--------|
| 1 | Policy and templates | `scope_template_required_sections`, `scope_registry_requires_new_specs`, `scope_existing_specs_are_grandfathered` |
| 2 | Complexity checker | Green / Yellow / Red 判定テスト 4 件 |
| 3 | Scope Lock | AC 一致・revision・vertical slice |
| 4 | Repository integration | `scope_checker_runs_in_verify` |

### Vertical Slice Gate

**Phase 1** は最小 Vertical Slice E2E（Green fixture が checker を通過）を先に通す。

Phase 1 成功前に実装してはならないもの:

- 追加 integration
- schema migration
- crash recovery
- 汎用 framework 化
- 性能最適化

### 実装中の禁止事項

実装中に新しい実行主体、状態機械、永続 aggregate、外部副作用、クラッシュ復旧が必要になった場合、そのまま追加実装してはならない。

`feature-scope.toml` と設計書を更新し、Complexity Gate を再判定すること。

## 2. 受け入れ条件

設計書 §9 を `scripts/spec-acceptance.toml` に登録済み。

## 3. 完了条件

1. 全 Phase の `pending = false`
2. `./scripts/verify.sh` 成功
3. 本ファイルを `docs/done/` へ移動し `0000_spec-index.md` を実装済みに更新

## 4. 仕様との差分

なし。
