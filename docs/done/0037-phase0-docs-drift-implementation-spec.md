# 0037 Phase 0 — docs/spec drift 修正 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §3 / §14 Phase 0  
> **状態**: 実装済み（Phase 0）  
> **起票**: 2026-06-13

## 目的

0037 実装着手前に、0034/0035 MVP 設計書と周辺 docs の **記述ドリフト** を解消する。コード変更は行わない。

## 受け入れ条件

1. `cwd` は `Option`（`absolute_path | null`）と明記。project scope の apply/query のみ cwd 必須
2. `MemoryContext` に `memory_space_id: string | null` が 0034/0035 の DTO 断片で統一されている
3. `status` に `open` が含まれ、`idea` は `status=open` と明記
4. identity 表現が `memory_space_id` 内の `kind + scope + project_key` に統一（`session_id + kind` 表現を残さない）
5. `AI_SESSION_ID` を memory owner とする古い記述が残っていない
6. `idea` 常時注入のような誤解を招く記述がない
7. 0034/0035 に 0037 への正本リンクと「矛盾時は 0037 優先」が記載されている
8. `docs/architecture.md` / `docs/security.md` / `docs/manual/contextual-memory.md` / index が 0037 と整合

## 変更対象

| ファイル | 変更内容 |
|----------|----------|
| `docs/spec/0034_aibe-contextual-memory-spec.md` | 0037 リンク、MemoryApply/Query context、identity 表現 |
| `docs/spec/0035_aibe-memory-identity-split-spec.md` | 0037 リンク、MemoryContext、MemoryEntry.status、cwd 説明 |
| `docs/architecture.md` | 0037 参照、idea on-demand 明記 |
| `docs/security.md` | idea 常時注入しない旨 |
| `docs/manual/contextual-memory.md` | 0037 参照、期待結果の補足 |
| `docs/0000_spec-index.md` | Phase 0 実装指示書の追記 |
| `docs/spec/README.md` | 必要なら Phase 0 完了注記 |

## 実装手順

1. 0034 ヘッダに 0037 を関連リンク追加。§0.1 で「0037 が v1 正式正本、矛盾時は 0037 優先」を追記
2. 0034 の `MemoryApply` / `MemoryQuery` context 断片を `cwd: absolute_path | null, memory_space_id: string | null` に統一
3. 0034 の `Add` 説明で `session_id + kind` → `memory_space_id` 内の `kind + scope + project_key` に修正
4. 0035 ヘッダに 0037 追加。§0.1 で 0037 正本関係を追記
5. 0035 の `MemoryContext` を `cwd: absolute_path | null` に修正。cwd 説明を project 必須 / session・global は null 可に更新
6. 0035 の `MemoryEntry.status` に `open` を追加。`idea` は `open` と明記
7. architecture / security / manual に 0037 リンクと idea on-demand を追記
8. `./scripts/check-docs-consistency.sh` と `./scripts/verify.sh` を実行

## テスト

コード変更なし。品質ゲート:

```bash
./scripts/verify.sh
./scripts/smoke-mock.sh
```

## 完了時

- 本ファイルを `docs/done/0037-phase0-docs-drift-implementation-spec.md` へ移動
- `docs/0000_spec-index.md` の tasks → done を更新
