# ai スマート入口 手動検証

`ai '...'` の smart entry（`route_turn` / `AI_SESSION_ID` / conversation 継続）の確認手順。

## 前提

```bash
cargo build -p aibe -p ai -p aish
export PATH="$PWD/target/debug:$PATH"
```

`aibe` 設定に `[router] profile = "fast"` があること（例: `docs/aibe.config.example.toml`）。

## 1. 標準フロー（`ai '...'`）

1. `aish shell` を起動し、`echo $AI_SESSION_ID` で session ID が export されていることを確認する。
2. 同一 shell 内で `ai 'hello'` を実行する。
3. 2 回目以降の `ai '...'` で会話が継続されること（stderr に継続通知が出る場合あり）。
4. `~/.local/share/aibe/conversations/<AI_SESSION_ID>/` に store が作られること。

## 2. 新規会話（`ai --new '...'`）

1. 上記 shell で `ai --new 'fresh start'` を実行する。
2. stderr に新規 conversation への切替が出ること。
3. 以降の `ai '...'`（`--new` なし）は新 conversation 内で継続すること。

## 3. 複数 tab での session 共有

1. 同一 `aish shell` セッションから 2 つの tab を開く（`AI_SESSION_ID` が同じ）。
2. tab A で `ai 'first tab'`、tab B で `ai 'second tab'` を実行する。
3. 両方が同一 conversation store を参照し、文脈が共有されること。

## 4. non-TTY fallback

```bash
echo hello | ai
ai 'hello' </dev/null
```

- `route_turn` を呼ばず従来の 1 shot ask になること。
- `AI_SESSION_ID` は request context に載ること（integration: `ai/tests/phase_a_cli.rs`）。

## 5. CLI 明示値の優先

```bash
ai --preset fast --tools read_file 'list files'
```

- `route_turn` は実行されるが、`--preset` / `--tools` が RoutePlan より優先されること。

## 6. route fallback

1. `aibe` を止めた状態、または router profile 未設定で `ai '...'` を実行する。
2. 1 回リトライ後、stderr に fallback 通知が出ること。
3. `tools=[]` の text-only 1 shot で応答が返ること。

## 7. shell 承認と `--yes-exec`

1. `shell_exec_approval=ask` で tool 付き ask を実行し、承認プロンプトが出ること。
2. 同一 session 内で `--yes-exec` を付けると承認が省略されること。
3. `shell_exec_approval=never` では `--yes-exec` でも実行されないこと。

## 期待結果

- TTY の `ai '...'` は常に smart entry（v1 opt-out なし）。
- 会話の正本は aibe の conversation store。`ai` local history は索引のみ。
- `route_reason` は path mask 済みで stderr / history に残る。
