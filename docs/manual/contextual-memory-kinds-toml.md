# kinds.toml サンプル

user-defined **MemoryKindRegistry** の設定例。built-in 6 kind（`goal` / `now` / `rule` / `decision` / `idea` / `note`）をベースに、サーバ全体または memory space 単位で拡張・上書きする。

設計正本: [spec/0037 §6.5](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md)（アーキテクチャ要約: [architecture.md](../architecture.md) contextual memory 節）。

## パス

`AIBE_ROOT` は通常 `~/.local/share/aibe`（`conversations/` の親）。

```text
<AIBE_ROOT>/memory/kinds.toml
<AIBE_ROOT>/memory/spaces/<memory_space_id>/kinds.toml
```

ファイルが無くても built-in のみで動作する。変更は次回 RPC / turn で再読み込み（filesystem watch はしない）。

## 読み込み順

```text
builtin → memory/kinds.toml → memory/spaces/<id>/kinds.toml
```

後勝ち。`memory_kind_list` / `MemoryApply` / resolver / `ClearKind` は **effective registry**（`memory_space_id` 基準）を使う。

## サンプル 1: サーバ全体で `goal` の説明と優先度を調整

`~/.local/share/aibe/memory/kinds.toml`:

```toml
[kinds.goal]
description = "チーム全体の north star（最終目的）"
aliases = ["goal", "north star", "最終目的", "ゴール"]
prompt.priority = 5
```

確認:

```bash
ai mem kinds
# goal の description / priority が上書きされていること
```

## サンプル 2: context 専用で `goal` をさらに上書き

`ctx_team` 用の space-local 設定:

```bash
mkdir -p ~/.local/share/aibe/memory/spaces/ctx_team
```

`~/.local/share/aibe/memory/spaces/ctx_team/kinds.toml`:

```toml
[kinds.goal]
description = "ctx_team 専用の目標"
aliases = ["goal", "チーム目標"]
```

確認:

```bash
AIBE_CONTEXT_ID=ctx_team ai mem kinds
# description が「ctx_team 専用の目標」になること（server より space-local が優先）
```

## サンプル 3: custom kind `checklist` を追加

サーバ全体に新 kind を定義:

```toml
[kinds.checklist]
description = "作業チェックリスト（未完了項目）"
default_scope = "project"
default_inject = "manual"
default_status = "open"
lifecycle = "open_archive"
cardinality = "multiple"
clear_from = "open"
clear_to = "archived"
aliases = ["checklist", "チェック", "todo"]

[kinds.checklist.prompt]
on_demand = true
priority = 85
max_entries = 20
keywords = ["checklist", "チェックリスト", "todo", "未完了"]
```

使い方:

```bash
ai mem add checklist "verify.sh を通す"
ai mem add checklist "docs を同期する"
ai mem show
# open な checklist が一覧・prompt block に反映されること（on-demand query 時）
```

`lifecycle = "open_archive"` の kind は prompt block 内で `- テキスト` のリスト形式になる。

## サンプル 4: `idea` の on-demand keywords をチューニング

built-in `idea` のマッチ語だけ変更（挙動の根は変えない）:

```toml
[kinds.idea]
prompt.priority = 75
prompt.keywords = ["idea", "アイデア", "ブレスト", "候補", "mvp"]
prompt.max_entries = 16
```

確認:

```bash
ai mem show --query "ブレストを整理して"
# open idea が on-demand で候補に入ること
```

## サンプル 5: `rule` の aliases をプロジェクト用語に合わせる

```toml
[kinds.rule]
description = "このリポジトリの作業ルール"
aliases = ["rule", "ルール", "コーディング規約", "方針"]
prompt.max_entries = 12
```

## built-in override で変更できる / できない項目

### 変更できる

| 項目 | TOML 例 |
|------|---------|
| 説明 | `description = "..."` |
| 別名 | `aliases = ["goal", "目的"]` |
| on-demand キーワード | `prompt.keywords = ["mvp"]` |
| 注入優先度 | `prompt.priority = 30` |
| 注入件数上限 | `prompt.max_entries = 8`（`0` = 無制限） |
| stale | `stale = "session_changed"`（主に `now`） |

### 変更できない（書くと RPC error）

- `default_scope` / `default_inject` / `default_status`
- `lifecycle` / `cardinality` / `clear_from` / `clear_to`
- `prompt.auto_inject` / `prompt.on_demand`

代表的な禁止例:

```toml
# 以下はいずれも merge 時に拒否される

[kinds.goal]
cardinality = "multiple"        # goal は single_effective 固定

[kinds.now]
default_scope = "project"       # now は session 固定

[kinds.idea]
default_inject = "pinned"       # builtin の inject 変更不可

[kinds.rule]
prompt.auto_inject = false      # auto_inject の変更不可
```

## kind ID の制約

- 英数字と `_` `.` `-` のみ（スペース不可）
- 空文字不可

```toml
# 無効
[kinds."bad kind"]
```

## parse 失敗時の挙動

| 経路 | 壊れた kinds.toml |
|------|-------------------|
| `ai mem kinds` / `ai mem add` / `ai goal set` 等 | **error**（`invalid_request`） |
| `ai mem show`（prompt block 付き） | **error** |
| `ai "..."`（AgentTurn 注入） | **best-effort**: built-in に fallback、memory block なしで turn 継続（サーバログに warn） |

意図的に壊して確認する例:

```toml
[kinds.goal]
default_scope = "not_a_scope"
```

```bash
ai mem kinds
# kind registry parse 系の error になること

ai "hello"
# turn 自体は失敗しないこと（注入なし）
```

## 手動検証チェックリスト

1. サンプル 1 を配置 → `ai mem kinds` で `goal.description` が変わること
2. サンプル 2 を追加 → `AIBE_CONTEXT_ID=ctx_team ai mem kinds` で space-local が優先されること
3. サンプル 3 を配置 → `ai mem add checklist "..."` が成功し `ai mem kinds` に `checklist` が出ること
4. 禁止例 TOML → `ai mem kinds` が error になること
5. 禁止例のまま `ai "query"` → turn は成功し、注入は built-in 相当または空であること

関連: [contextual-memory.md](contextual-memory.md)（CLI 手動検証）、[contextual-memory-multi-client.md](contextual-memory-multi-client.md)（multi-client / subscribe）。
