# contextual memory 手動検証

`ai goal` / `ai now` / `ai idea` / `ai mem` と、`AgentTurn` への自動注入を確認する。

## 前提

```bash
cargo build -p aibe -p ai
export PATH="$PWD/target/debug:$PATH"
# mock または本番 aibe 設定
```

`AI_SESSION_ID` は `aish shell` から export されるか、`ai` が自前生成する。

## 手順

1. `ai goal set "AIBEに文脈付き記憶レイヤーを作る"` — `goal set:` が表示されること。
2. `ai now set "まず MemoryApply / MemoryQuery を実装する"` — `now set:` が表示されること。
3. `ai idea add "Context Card をユーザー定義にしたい"` — `idea added:` が表示されること。
4. `ai goal show` / `ai now show` / `ai idea list` — 保存内容が TSV で見えること。
5. `ai mem show` — `[aibe contextual memory]` を含む prompt block が表示されること（`--query "今あるideaからMVPを整理して"` で idea も含むこと）。
6. 同じ `AI_SESSION_ID` の別ターミナルから `ai goal show` — 同じ goal が見えること。
7. `ai "次にどこから実装すべき？"` — LLM 側（mock ログ等）で `[aibe contextual memory]` に goal / now が入り、idea は入らないこと。
8. `ai "今あるideaからMVPを整理して"` — open idea も注入されること。

## 期待結果

- memory の正本は `~/.local/share/aibe/conversations/<AI_SESSION_ID>/memory/events.jsonl`（aibe 側）。
- `aish` は変更なし。`ai` は memory をローカル保持しない。
- memory は system instruction ではなく user-maintained context block として注入される。
