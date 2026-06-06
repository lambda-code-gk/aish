# CLI サブエージェント — 製品調査（旧案・歴史資料）

> **調査日**: 2026-06-03  
> **環境**: Codex CLI `0.133.0`（`codex --version`）。**Claude Code は未インストール** — 公式ドキュメントベース。
> **注記**: 本調査は 0024 / 0025 の first-class 統合案の参考資料。採用方針は [0026_external-commands-spec.md](../spec/0026_external-commands-spec.md) を正とする。

## 目的

aibe の CLI サブエージェントアダプタが呼ぶコマンド・フラグ・出力パースを調べた記録。現在の採用方針は外部コマンド（0026）であり、本書は歴史資料として扱う。

## Codex CLI（実機 help + OpenAI 公式）

### サポート最小バージョン

- **0.133.0**（本環境）。JSONL イベント形は [Non-interactive mode](https://developers.openai.com/codex/noninteractive) の `thread.started` / `item.*` 系（legacy `--json` 形式は廃止方向 — [openai/codex#4525](https://github.com/openai/codex/issues/4525)）。

### 初回実行（形態 A / B 共通のベース）

```bash
codex exec \
  --json \
  -C "<context.cwd>" \
  -s workspace-write \
  --skip-git-repo-check \
  -o "<tmp>/last-message.txt" \
  "<user prompt>"
```

| フラグ | 用途 |
|--------|------|
| `--json` | stdout を JSONL（`thread.started`, `item.completed`, `turn.completed` 等） |
| `-C` / `--cd` | 作業ルート（aibe の `context.cwd`） |
| `-s workspace-write` | ファイル編集を許可（MVP「編集まで含む」） |
| （非対話） | Codex CLI **≥0.133** では `--ask-for-approval` は**廃止**。aibe は付けない。`-s` + stdin 閉じ + ユーザーの `~/.codex/config.toml` / `-p` で承認を抑える |
| `--skip-git-repo-check` | cwd が git でない場合の起動（必要時のみ） |
| `-o` / `--output-last-message` | 最終自然言語をファイルにも書く（`summary_text` の補助） |
| `-p` / `--profile` | `$CODEX_HOME` の Codex プロファイル（aibe `[llm.*]` の `codex_profile` と対応） |
| `--ephemeral` | セッション永続化不要の一回限り |

**Landlock（本リポジトリ Linux）**: 起動ラッパーで `codex --enable use_legacy_landlock` を付与（[scripts/codex-cli.sh](../../scripts/codex-cli.sh)）。

**認証**: 対話 `codex login` 済みの `~/.codex/auth.json` を再利用。CI 向け `CODEX_API_KEY` は **`codex exec` のみ**（公式 Non-interactive 節）。

### resume

```bash
codex exec resume <THREAD_ID> --json --skip-git-repo-check "<follow-up>"
# または
codex exec resume --last --json "..."
```

- ID は JSONL の `{"type":"thread.started","thread_id":"..."}`（**フィールド名は `thread_id`**。aibe artifacts の `thread_id` に格納）。
- **Codex CLI ≥0.133**: `exec resume` には **`-C` / `-s` / `-p` は付けない**（初回 `exec` のみ）。aibe は resume 時に `--json` と `--skip-git-repo-check` とプロンプトのみ渡す。
- resume は **cwd セッションと紐づく**（`--last` は同一 cwd の最新）。aibe は子プロセスの `current_dir` を `context.cwd` に揃える。

### stdout の扱い（aibe パース）

| artifacts フィールド | 取得元 |
|---------------------|--------|
| `thread_id` | 最初の `thread.started` |
| `summary_text` | 最後の `item.completed` で `item.type` / `item_type` が `agent_message` / `assistant_message` の `text`、または `-o` ファイル |
| `exit_status` | プロセス終了コード。`turn.failed` / `error` 行があればエラー種別をログに残す |
| `changed_files` | `item.completed` の `file_change`（公式: item types に file change あり）— パスを重複除去して列挙 |

**非 `--json` モード**: 最終メッセージのみ stdout（進捗は stderr）。0024 は **機械可読必須のため `--json` 固定**。

### 検証メモ（本環境）

- サンドボックス／タイムアウト下の実実行は未完了（要ネットワーク・認証・対話 stdin の影響）。help と公式ドキュメントで契約を固定し、CI はフェイク JSONL で検証する。

---

## Claude Code CLI（公式ドキュメントのみ）

### バイナリ

- コマンド名: **`claude`**（[Run Claude Code programmatically](https://code.claude.com/docs/en/headless)）。
- 本環境: **未インストール** — 手動検証・起動時チェックはスキップ。実装後に別マシンで `docs/manual/cli-subagent-products.md` の手順を追記する。

### 初回実行

```bash
claude -p "<user prompt>" \
  --output-format json \
  --permission-mode acceptEdits \
  --allowedTools "Read,Edit,Bash"
```

| フラグ | 用途 |
|--------|------|
| `-p` / `--print` | 非対話（headless） |
| `--output-format json` | 単発 JSON（`result`, `session_id`, `is_error`, `total_cost_usd` 等） |
| `--permission-mode acceptEdits` | ファイル編集をプロンプトなしで許可（ドキュメント推奨パターン） |
| `--allowedTools` | 必要ツールの明示（編集・シェル） |
| `--bare` | ローカル hooks/MCP/CLAUDE.md 自動読込を抑え、CI 向け再現性（任意） |

**認証**: サブスク／`ANTHROPIC_API_KEY` / `claude login` — aibe は秘密を保持しない。未ログイン時は **aibe 起動失敗**（0024）。

### resume

```bash
session_id=$(claude -p "..." --output-format json | jq -r '.session_id')
claude -p "..." --resume "$session_id" --output-format json
```

- **`--continue` は非対話では不安定**（新セッションになることがある）— **`--resume <session_id>` 必須**（複数コミュニティ記事・公式例と一致）。
- resume は **セッション作成時と同じ cwd** が推奨（トランスクリプト参照）。

### stdout の扱い（aibe パース）

| artifacts フィールド | 取得元 |
|---------------------|--------|
| `thread_id` | JSON の **`session_id`**（aibe 側の名前は `thread_id` のまま統一） |
| `summary_text` | `.result`（`is_error == true` ならエラー） |
| `exit_status` | プロセス終了コード + `is_error` |
| `changed_files` | **単発 `json` だけではファイル一覧が保証されない** → 初版アダプタは `--output-format stream-json --verbose` を使い、ツール／編集イベントからパスを抽出する方針（実装時にイベント形をスナップショットテストで固定）。取れない場合は **空配列**（git フォールバックなし） |

### バージョン

- ドキュメント上、stdin 10MB  cap 等は **v2.1.128** 言及 — 実装時に `claude --version` で下限を 0024 に追記する。

---

## aibe 起動時チェック（両製品）

| チェック | 失敗時 |
|----------|--------|
| `command` が PATH で実行可能 | `ConfigError`（aibe 起動失敗） |
| Codex: 任意で `codex doctor` は手動のみ | — |
| Claude: `claude --version` | 未インストール環境では Claude 用 `[llm.*]` を設定しない |

---

## 手動スモーク（実 CLI がある環境向け）

### Codex

```bash
export MANUAL="$(mktemp -d)"
cd /path/to/git/repo
codex exec --json -s workspace-write \
  "List three files in the repo root" -o "$MANUAL/last.txt" | tee "$MANUAL/events.jsonl"
grep thread.started "$MANUAL/events.jsonl"
test -s "$MANUAL/last.txt"
```

### Claude Code（要 `claude` インストール）

```bash
claude -p "List three files in the repo root" --output-format json | jq .
```

---

## 関連

- [0024_cli-subagent-provider-spec.md](../0024_cli-subagent-provider-spec.md)
- [codex-delegation.md](../codex-delegation.md)（Cursor MCP — 別経路）
