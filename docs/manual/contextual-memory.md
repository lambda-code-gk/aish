# contextual memory 手動検証

`ai goal` / `ai now` / `ai idea` / `ai mem` / `ai context` と、`AgentTurn` への自動注入を確認する。Phase 2 以降は `ai mem kinds` と registry defaulting による `ai mem add rule|decision|note` も対象。

設計正本: [spec/0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md)（MVP 背景: [0034](../spec/0034_aibe-contextual-memory-spec.md) / [0035](../spec/0035_aibe-memory-identity-split-spec.md)）。

## 前提

```bash
cargo build -p aibe -p ai
export PATH="$PWD/target/debug:$PATH"
# mock または本番 aibe 設定
```

- `AI_SESSION_ID` は runtime session（`aish shell` export または `ai` 自前生成）。**memory の owner ではない**。
- contextual memory の owner は `memory_space_id`（**クライアント**の `AIBE_CONTEXT_ID` または `ai context use/new`、または project 自動導出）。サーバ `aibe` は `AIBE_CONTEXT_ID` を読まない。

## 手順

1. `ai context current` — 解決された `memory_space_id` と source が表示されること。
2. `ai context use ctx_a` — config に current context が保存されること（`AIBE_CONTEXT_ID` があればそちらが優先）。
3. `AI_SESSION_ID=sess_001 ai goal set "AIBEに文脈付き記憶レイヤーを作る"` — `goal set:` が表示されること。
4. `AI_SESSION_ID=sess_001 ai now set "まず MemoryApply / MemoryQuery を実装する"` — `now set:` が表示されること。
5. `AI_SESSION_ID=sess_002 ai context use ctx_a` のうえで `ai goal show` — **sess_001 と同じ goal** が見えること（session が違っても memory space が同じ）。
6. `AI_SESSION_ID=sess_002 ai now show` または `ai mem show` — `now` は見えるが **stale** 表示があること（別 session で更新されていないため）。
7. `AI_SESSION_ID=sess_003 ai context use ctx_b` のうえで `ai goal show` — `ctx_a` の goal は**見えない**こと。
8. `ai idea add "Context Card をユーザー定義にしたい"` — `idea added:` が表示されること。
9. `ai mem show` — `memory_space_id:` の行（current context）と `[aibe contextual memory]` を含む prompt block が表示されること。
10. `ai mem kinds` — built-in 6 kind（`goal` / `now` / `rule` / `decision` / `idea` / `note`）が一覧表示されること。`--format env` では `kinds[0].id='goal'` 形式になること。
11. `AIBE_CONTEXT_ID=ctx_a ai mem add rule "idea は通常クエリへ常時注入しない"` — `mem add rule:` が表示され、次の `ai mem show` の prompt block に `[rule]` が含まれること。
12. `AIBE_CONTEXT_ID=ctx_a ai mem add custom "メモ"` — unregistered kind も `kind + text` のみで追加できること（server が `project/manual/open` で補完）。
13. `ai context use ctx_a` に戻したうえで `ai "次にどこから実装すべき？"` — LLM 側で **ctx_a の goal / now / rule** が注入され idea は通常入らないこと（turn も current context に従う）。

## 期待結果

- memory の正本は `~/.local/share/aibe/memory/spaces/<memory_space_id>/events.jsonl`（aibe 側）。
- 0034 以前の `conversations/<AI_SESSION_ID>/memory/events.jsonl` は read-through / lazy copy で互換（破壊しない）。
- `aish` は変更なし。`ai` は memory をローカル正本として保持しない（`[context] current` の名前のみ config に保存）。
- memory は system instruction ではなく user-maintained context block として注入される。
- `idea` は通常クエリへ常時注入されない（on-demand のみ）。通常 turn では **goal / now / rule**（active）が pinned 注入される。
- `ai mem add` は `kind + text` のみ送る。scope/inject/status の defaulting は **AIBE server** が行う（`ai` は policy を持たない）。
