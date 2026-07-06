# 0056 — Feature Scope Governance / Complexity Gate 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-07-06  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)、[`AGENTS.md`](../../AGENTS.md)

## 0. Core outcome

新規 feature の複雑性と scope 変更を、実装前および CI で機械的に検査し、危険な scope 膨張を止める。

## 1. Minimum vertical slice

```text
新規 spec（0057 相当の fixture）を feature-scope.toml に登録
→ 設計書に必須節を記載
→ check-feature-scope.py が Green を受理
→ spec-acceptance.toml と locked_ac_ids が一致
→ verify.sh 経由で checker が実行される
```

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。checker はリポジトリ内の TOML / Markdown / git 履歴のみを読む。

### 2.2 保証対象外

- 工数・開発日数の自動見積もり
- ソースコードからの状態機械自動推論
- AI による仕様自動分割
- PR の自動承認・拒否
- 過去 spec（0026–0055）の一括移行

## 3. Non-goals

- GitHub Project 連携
- 複雑性スコアの精密数理モデル
- LOC / ファイル数による品質判定
- 新しい Rust crate・DB・Web UI
- 外部 Python package 依存

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（CI / 開発者が checker を実行） |
| 状態機械 | 0 |
| 永続 aggregate | 0（TOML レジストリのみ） |
| 外部副作用 | 0（git show の読み取りのみ） |
| プロセス境界 | 0 |
| 新規基盤機構 | feature-scope-governance |
| 他機能統合 | spec-acceptance、verify |

## 5. Complexity Gate

- 判定: Red（governance 機構）
- 理由: 複数検査責務を束ねるが、runtime 状態機械・永続化は持たない
- 分割判断: governance 機構自体のため単一 spec として実装する
- 承認例外: spec 0056 は scope_class=governance として本書で承認

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| 新規基盤機構 | +0（governance 自体が唯一の機構） |
| Red flag フィールド | +0 |

## 7. Split triggers

次が必要になったら別 spec へ分割する。

- ソースコード AST からの複雑性推論
- GitHub API 連携
- 自動 spec 分割提案

## 8. パック構成の適用

**No** — 開発フロー・CI 検査であり、runtime の optional 機能ではない。core クレートへ組み込まない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `scope_template_required_sections` | 設計書テンプレートに必須節が揃っている |
| `scope_registry_requires_new_specs` | enforce_from_spec 以降の設計書に registry entry が必要 |
| `scope_checker_accepts_valid_green_spec` | Green fixture が checker を通過する |
| `scope_checker_classifies_yellow` | Yellow fixture が適切に分類される |
| `scope_checker_rejects_red_feature` | Red な通常 feature が拒否される |
| `scope_checker_requires_yellow_review` | Yellow で review 不足が拒否される |
| `scope_lock_matches_acceptance_ids` | locked AC 集合と acceptance registry が一致 |
| `scope_revision_required_on_locked_change` | Lock 後の scope 変更に revision 増加が必要 |
| `scope_vertical_slice_required` | vertical_slice_ac_id が必須 |
| `scope_existing_specs_are_grandfathered` | 0026–0055 は registry 未登録でも通過 |
| `scope_checker_runs_in_verify` | verify.sh が checker を実行する |

## 10. Deferred specs

- ソースコード複雑性の自動推論（別 spec 候補）
- 過去 spec の一括 migration（意図的に非対象）

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | 初版 scope lock | 0056 実装開始 |
