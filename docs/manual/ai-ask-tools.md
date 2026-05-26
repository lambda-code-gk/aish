# ai ask ツール allowlist 手動検証

`docs/done/0002_ai-tools-client-spec.md` の CLI / config / 表示契約を、実バイナリで確認する。

自動テスト（`ai/tests/ask_integration.rs` 等）でカバーしていない **起動時 `stderr` 1 行** と、**実 aibe 経由の入出力分離** を主目的とする。

## 前提

- Unix（Linux 等）
- ビルド済み:
  ```bash
  cargo build -p aibe -p ai
  ```

### 自動スモーク（CI と同じ契約）

手動の B1（`tools enabled: none` + mock 応答）に相当する導通は、次で一括確認できる（`AIBE_CONFIG` / `AIBE_SOCKET_PATH` / `AI_CONFIG` は一時ディレクトリに隔離され、ホームの本番設定は読まない）。スクリプトは stdout / stderr の非空行が各 1 行かつ全文一致、`warning:` 行なしを検証する:

```bash
./scripts/smoke-mock.sh
```

仕様: [0014_ci-smoke-stabilization-spec.md](../done/0014_ci-smoke-stabilization-spec.md)。
- 作業用ディレクトリ（以降 `$MANUAL`）:
  ```bash
  export MANUAL="$(mktemp -d)"
  echo "MANUAL=$MANUAL"
  ```

## 共通: 隔離用設定（mock aibe）

実 API キーは不要。`$MANUAL` にだけ設定を置く。

```bash
export AIBE_CONFIG="$MANUAL/aibe.toml"
export AIBE_SOCKET_PATH="$MANUAL/aibe.sock"
export AI_CONFIG="$MANUAL/ai.toml"

cat >"$AIBE_CONFIG" <<'EOF'
[llm]
provider = "mock"

[tools.read_file]
allowed_roots = ["."]
EOF

cat >"$AI_CONFIG" <<'EOF'
[ask]
tools = "@read-only"
EOF

touch "$MANUAL/sample.txt"
echo 'hello from manual fixture' >"$MANUAL/sample.txt"
```

## A. 検証のみ（aibe 未接続・未起動）

`--no-start` により、解決エラー時は **aibe を起動しない**（`ensure_running` も呼ばれない）。

| # | コマンド | 期待 |
|---|----------|------|
| A1 | `cargo run -q -p ai -- ask "x" --tools nope --no-start 2>&1` | 終了コード非 0。`unknown tool: nope` を含む。`tools enabled` は出ない |
| A2 | `cargo run -q -p ai -- ask "x" --tools none,read_file --no-start 2>&1` | 非 0。`none cannot be combined` を含む |
| A3 | `cargo run -q -p ai -- ask "x" --tools @nope --no-start 2>&1` | 非 0。`unknown tool category` を含む |

確認例:

```bash
cargo run -q -p ai -- ask "x" --tools nope --no-start 2>&1 | tee "$MANUAL/a1.log"
test $? -ne 0
grep -q 'unknown tool: nope' "$MANUAL/a1.log"
```

## B. mock aibe 経由（起動時行・stdout 分離）

### B0. aibe 起動

ターミナル 1:

```bash
# 上記「隔離用設定」を export 済みであること
rm -f "$AIBE_SOCKET_PATH"
cargo run -q -p aibe -- -f
```

`aibe: listening on ...` が出ること。検証後はターミナル 1 で Ctrl+C、または `kill <aibeのPID>` で止める。

### B1. 既定 `[]`（config も CLI も tools なし）

一時的に `AI_CONFIG` を空の ask 相当にするか、別ファイルで `[ask]` を省略:

```bash
cat >"$MANUAL/ai-empty.toml" <<'EOF'
# [ask] 省略 → tools []
EOF
AI_CONFIG="$MANUAL/ai-empty.toml" \
  cargo run -q -p ai -- ask "ping" --socket "$AIBE_SOCKET_PATH" --no-start 2>"$MANUAL/b1.err" \
  | tee "$MANUAL/b1.out"
```

期待:

- **stderr**: `ai: tools enabled: none`（`warning:` なし）
- **stdout**: `[mock] received: ping` のみ（ツール詳細行なし）

### B2. CLI `--tools @read-only`

```bash
cargo run -q -p ai -- ask "hello tools" \
  --socket "$AIBE_SOCKET_PATH" --no-start \
  --tools @read-only 2>"$MANUAL/b2.err" | tee "$MANUAL/b2.out"
```

期待:

- **stderr**: `ai: tools enabled: read_file (@read-only)`（`warning:` なし）
- **stdout**: mock 応答 1 行のみ

### B3. `shell_exec` 警告行

```bash
cargo run -q -p ai -- ask "x" \
  --socket "$AIBE_SOCKET_PATH" --no-start \
  --tools @exec 2>"$MANUAL/b3.err" | tee "$MANUAL/b3.out"
```

期待:

- **stderr** 先頭行: `warning: ai: tools enabled: shell_exec (@exec)`

### B4. config 上書き `--tools none`

`AI_CONFIG` は `@read-only` のまま:

```bash
cargo run -q -p ai -- ask "override" \
  --socket "$AIBE_SOCKET_PATH" --no-start \
  --tools none 2>"$MANUAL/b4.err" | tee "$MANUAL/b4.out"
```

期待:

- **stderr**: `ai: tools enabled: none`
- mock は tools なしで応答（aibe ログで `tools: []` を確認してもよい）

### B5. `--verbose-tools` と stdout の分離（mock）

mock LLM はツールを呼ばないため、`tool_calls` 詳細行は **出ない** のが正常。契約確認は次のとおり:

```bash
cargo run -q -p ai -- ask "verbose check" \
  --socket "$AIBE_SOCKET_PATH" --no-start \
  --tools @read-only --verbose-tools 2>"$MANUAL/b5.err" | tee "$MANUAL/b5.out"
```

期待:

- **stdout**: 最終 assistant 本文のみ（`ai: tool` で始まる行がない）
- **stderr**: 起動時 1 行 +（mock では tool 詳細行なし）

`tool_calls` 詳細・切り詰め・`max_tool_rounds` 警告は統合テスト `presenter_max_tool_rounds_and_verbose_tools_contract` で担保。実ツール出力の確認は **C** を参照。

## 相対パスとカレントディレクトリ

**方針（全ツール共通）**: 相対パス・`.` 付き許可ルートは **aibe デーモンの cwd ではなく `ai` を実行したディレクトリ**（`context.cwd`）を基準にする。ツール有効時は `context.cwd`（絶対パス）が **必須**（未送信は `invalid_request`）。詳細は `docs/done/0003_architecture-review-refactor-spec.md` と `docs/architecture.md`。

`read_file` / `shell_exec` の確認例（相対パス・相対引数は `cd` したディレクトリ基準）:

確認例（mock aibe・B0 起動済み）:

```bash
cd "$MANUAL"
echo 'cwd fixture' >./cwd-test.txt
cargo run -q -p ai -- ask "x" \
  --socket "$AIBE_SOCKET_PATH" --no-start \
  --tools @read-only 2>/dev/null
# 実 LLM で read_file させる場合は C のプロンプトで path を "cwd-test.txt" にする
```

## C. 実 LLM + `read_file`（任意・API キー要）

[aibe-openai-compatible.md](aibe-openai-compatible.md) と同様に `~/.config/aibe/config.toml` で `provider = "openai_compatible"` を有効にする（OpenAI 公式 API も同じ provider 名）。

追加で aibe 側:

```toml
[tools.read_file]
allowed_roots = ["."]   # または検証用ディレクトリのみ
```

手順:

1. aibe を `-f` で起動（本番 socket または `AIBE_SOCKET_PATH`）
2. 読めるファイルを 1 つ用意（例: `$MANUAL/sample.txt`）
3. ターミナル 2:
   ```bash
   cd "$MANUAL"
   cargo run -q -p ai -- ask \
     "Read the file sample.txt with read_file and reply with only the file contents." \
     --tools @read-only --verbose-tools 2>ai-tools-verbose.err | tee ai-tools.out
   ```

期待:

- **stdout**: ファイル内容に近い短い応答（モデル依存）
- **stderr**: `read_file` の `ai: tool ...` 行が **1 行以上**（`--verbose-tools`）
- **stdout** に `ai: tool` 行が **ない**
- ターミナル・ログに API キー・Bearer が出ない（`docs/security.md`）

失敗しやすい点:

- `allowed_roots` 外のパス → ツールエラーだが turn は継続しうる（0001 仕様）
- モデルがツールを呼ばない → C は再試行またはプロンプト調整

## D. `max_tool_rounds` 到達（任意・実 LLM）

既定（`termination_strategy` 未設定）は **SummaryPrompt** — 0003 と同じ要約経路。統合テスト `max_tool_rounds_returns_agent_turn_result_with_tool_calls` で mock 確認済み。

**ConversationReplay** を試す場合（プロバイダが tool role を plain `complete()` で解釈するときのみ有効）:

```toml
[tools]
max_rounds = 2   # 短く上限到達させる
termination_strategy = "conversation_replay"
```

手順（実 LLM・ターミナル 2）:

1. aibe を `-f` で起動
2. ツールを複数ラウンド呼ばせるプロンプト（例: `read_file` で 2 ファイル以上を順に読む）
3. 上限到達後:
   - **stdout**: 最終 assistant 本文（部分結果を含む）
   - **stderr**: `max_tool_rounds` warning 1 行（0002 Presenter 契約）
   - aibe 側ログ（tracing 有効時）: `termination strategy=...` 1 行

期待:

- `status: max_tool_rounds` の `agent_turn_result`（`type: error` ではない）
- Replay が provider error になった場合は SummaryPrompt に自動フォールバック（設定は `conversation_replay` のままでよい）

## 新規組み込みツール追加チェックリスト

`aibe` に組み込みツールを追加するとき、カテゴリ表と `KNOWN_TOOLS` のドリフトを防ぐ。仕様: `docs/done/0009_ai-tool-category-sync-spec.md`。カテゴリ表の仕様正本: `docs/done/0002_ai-tools-client-spec.md` §カテゴリ表。

**分類責務**: メンテナが新ツールを `@read-only` / `@exec` / `@full` のどれに含めるか判断する。`@full` は常に **全** `aibe::KNOWN_TOOLS` を展開すること。

1. **aibe** — `aibe/src/domain/tool_name.rs` の `KNOWN_TOOLS`、ツール定義・実装を追加
2. **ai 展開** — `ai/src/domain/tools.rs` の `expand_category` を更新（該当カテゴリに新名を含め、`@full` が全 KNOWN_TOOLS をカバーするようにする）
3. **仕様** — `docs/done/0002_ai-tools-client-spec.md` §カテゴリ表を更新
4. **テスト** — 次が成功すること:
   ```bash
   cargo test -p ai tool_catalog_sync
   cargo test -p ai tool_names_sync
   ```
5. **手動** — 本書 B 系（起動時 `stderr` 行）で必要なら追加ケースを確認

## 記録

実施日・実施者・結果をメモする場合の例:

| 区分 | 実施 | 結果 |
|------|------|------|
| A1–A3 | | |
| B1–B5 | | |
| C | 任意（実 LLM） | |

## よくある失敗

- `aibe.sock` が残存 / 権限 → `rm -f "$AIBE_SOCKET_PATH"` 後に再起動
- `AI_CONFIG` / `AIBE_CONFIG` 未 export → 本番 `~/.config` を読んでしまう
- `--no-start` 忘れ → 別 socket の本番 aibe に繋がる
- B で aibe 未起動 → 接続エラー（検証対象外。先に B0）

**C（実 LLM）は任意。** AI 未実施の完了報告では「C 未実施」と明記する。A・B は可能な限り実施する。

## 実施記録（リポジトリメンテナンス用）

| 区分 | 実施日 | 実施者 | 結果 |
|------|--------|--------|------|
| A1–A3 | 2026-05-23 | Cursor（自動実行） | OK |
| B1–B5 | 2026-05-23 | Cursor（mock aibe・隔離設定） | OK |
| C | 2026-05-23 | ユーザー（実 LLM・手順 C） | OK（問題なし） |
