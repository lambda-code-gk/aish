# 0042 — Configurable Smart Features 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-17  
> **関連**: [0041_ai-smart-feature-plan-spec.md](0041_ai-smart-feature-plan-spec.md)、[0039_aish-memory-pack-externalization-spec.md](0039_aish-memory-pack-externalization-spec.md)、[architecture.md](../architecture.md)

## 0. 目的

0041 で `FeatureAction` 実行基盤と `route_turn` → `ai feature_executor` 経路は入ったが、機能選択は LLM 任せで精度が低く、AISH 固有の feature 定義はコードに残っている。

本仕様の目的:

1. AIBE core は **FeatureAction 実行基盤** のみを持つ
2. AISH 固有の「エラー調査」「目的整理」「メモリ参照」などは **pack の `features.toml`** で定義する
3. `route_turn` プロンプトに action schema を明示し、LLM が `feature_actions` を返しやすくする
4. trigger マッチで registry 由来の action を **LLM 出力にマージ**する（LLM 漏れの補完）
5. 0041 レビュー指摘（log tail 上限、履歴 summary、integration test）を解消する

## 1. 非目標

- `ai` バイナリの再帰呼び出し
- 動的プラグインロード
- `recommended_tools`（従来 advisory）の挙動変更（`shell_exec` 含みうる互換を維持）
- Windows 対応

## 2. `features.toml` スキーマ

pack 内 `aibe/memory/packs/aish-memory/features.toml` を正本とする。

```toml
[inspect_error]
description = "直近のエラーや失敗原因を調べる"
triggers = ["エラー", "失敗", "動かない", "原因", "error", "failed"]

[[inspect_error.actions]]
type = "set_log_tail_bytes"
bytes = 20480

[[inspect_error.actions]]
type = "set_recommended_tools"
tools = ["read_file", "grep", "git_status"]

[clarify_goal]
description = "作業目的を整理する"
triggers = ["目的", "ゴール", "整理", "clarify"]

[[clarify_goal.actions]]
type = "memory_recipe_run"
recipe_id = "clarify-goal"
apply = false
```

- トップレベルキー = feature id
- `triggers` は user query への部分一致（大文字小文字無視）
- `actions` は `FeatureAction` wire と同型
- `apply=true` の action は registry 読み込み時に除外

## 3. 設定

`~/.config/aibe/config.toml` の `[memory]` に追加:

```toml
[memory]
feature_files = [
  "memory/packs/aish-memory/features.toml",
]
```

- `feature_files = None` → baseline pack（同梱 `features.toml`）を使用（`kind_files` と同様の互換）
- `feature_files = []` → registry 空（LLM + prompt schema のみ）

## 4. `route_turn` プロンプト

system / user prompt に以下を含める:

- 許可 action type 一覧と JSON 形状
- 各 action の使用タイミング
- 必須キー一覧に `feature_actions` を含める（空配列可）
- registry に登録された feature の `description` 一覧（参考情報）

## 5. registry マージ

`finalize_route_plan` 後:

1. query を registry の triggers と照合
2. マッチした feature の actions を `feature_actions` に追加
3. 既に同等 action がある場合は重複追加しない

## 6. `recommended_tools` と `SetRecommendedTools` の整理

| 経路 | 意味 | shell_exec |
|------|------|------------|
| `RoutePlan.recommended_tools` | 0030 互換 advisory | 含みうる（実行時承認あり） |
| `FeatureAction::SetRecommendedTools` | 0041 smart feature 自動適用 | **除外**（read-only のみ） |

## 7. log tail 上限

`feature_executor` の `SetLogTailBytes` は `SHELL_LOG_TAIL_MAX_BYTES` で clamp する。超過で turn 全体を失敗させない。

## 8. 履歴

feature executor が生成した system message の **全文は agent_turn にのみ渡す**。

local history:

- `request_messages` — replay 用 transcript（user / assistant の会話本文）
- `feature_summaries` — redacted summary のみ（例: `[smart feature: memory_query entries=3]`）

### 8.1 retry / rerun（TTY + ask）

`ai history retry` / `rerun` は TTY かつ元 turn が `ask` のとき、`route_turn` と feature executor を **再実行**する（保存済み transcript の replay ではなく、現行 registry / route で再計画）。

non-TTY または `chat` 等は従来どおり `request_messages` を replay する。

### 8.2 memory.enabled=false

- `aibe`: feature registry は `memory.enabled` に関係なくロードする（route_turn の trigger マージは有効）。
- `ai`: `memory_query` / `memory_recipe_run` は memory 無効時 no-op。`set_log_tail_bytes` / `set_recommended_tools` は有効。

### 8.3 memory_query 重複防止

registry マージ時、`MemoryQuery` の `user_query` は executor が user input から補完するため、重複判定から除外する。

## 9. テスト

| 種別 | 内容 |
|------|------|
| unit | registry trigger マッチ、action パース、log tail clamp、shell_exec 除外 |
| integration | `route_turn` feature_actions → feature_executor → agent messages |
| aibe | route prompt に schema が含まれること、registry マージ |

## 10. 受け入れ条件

- baseline `features.toml` が読み込まれ、trigger マッチで `feature_actions` が補完される
- `route_turn` プロンプトに action schema が含まれる
- `SetLogTailBytes` が上限内に clamp される
- history に memory 全文が残らない（`feature_summaries` に summary のみ）
- `./scripts/verify.sh` と `./scripts/smoke-mock.sh` が通る
