# aish 対話シェル + JSONL ログ 手動検証

## 前提

- `cargo build -p aish` / `cargo build -p ai`
- ターミナルが TTY であること（パイプのみの stdin では対話できない）

## 環境変数（`ai ask`）

| 変数 | 設定者 | 内容 |
|------|--------|------|
| **`AISH_SESSION_DIR`** | `aish shell`（子シェルへ **絶対パス** で export） | `<log_dir>/<12桁hex>/`。`ai` が使う唯一のセッション変数 |
| **`AI_ASK_LOG=session`** | ユーザー（任意） | 上記 dir の `current_log` を tail して aibe に送る |

**`--session <id>` のルール**

1. **`AISH_SESSION_DIR` が必須**（未設定ならエラー）
2. **`<id>` は `basename "$AISH_SESSION_DIR"` と一致**（例: `echo "$AISH_SESSION_DIR" | xargs basename`）
3. `ai` は `current_log` を解決し、**symlink 先が session dir 内の読み取り可能な通常ファイル**であることを検証してから tail する

**別ペイン（B）**

```bash
export AISH_SESSION_DIR="$HOME/.local/share/aish/sessions/<12桁hex>"   # 絶対パス
ai ask --session <12桁hex> "…"
```

## セッション layout（0019）

`aish shell` 起動時:

1. セッション親ディレクトリ: `~/.config/aish/config.toml` の `log_dir`（未設定時は `~/.local/share/aish/sessions`）
2. `<log_dir>/<12桁hex>/log.jsonl` と `current_log` → `log.jsonl` が作成される
3. `stderr` に `aish: session <id> (dir …)` が表示される
4. 子シェルに **`AISH_SESSION_DIR`**（セッション dir の絶対パス）が export される（`AI_ASK_LOG` は export しない）

## CLI 共通オプション（`aish`）

| オプション | 意味 |
|-----------|------|
| `--format tsv\|json\|env` | **情報表示系**サブコマンド向けの出力形式（既定 `tsv`） |

- **全サブコマンド**（`exec`, `shell`, `session` 等）で指定可能。未知の値はエラー
- **情報表示系**のみ stdout の形式に使う。現状は **`session` のみ**
- **実行系**（`exec`, `shell`）は現状 `--format` を出力に反映しない（将来追加する情報表示系と CLI を揃えるため **受理のみ**）
- 形式の意味（情報表示系で共通）:
  - `tsv` — `key\tvalue` 行
  - `json` — JSON オブジェクト
  - `env` — `KEY='value'` 行（`eval` 向け）

正本: [docs/architecture.md](../architecture.md)（aish CLI / 共通 `--format` 節）

## 手順（セッション情報）

`session` は共通 `--format` を使う最初の情報表示系サブコマンド。`aish shell` 内で:

```bash
aish session                    # 既定 tsv（stdout）
aish session --format json
aish session --format env       # eval "$(aish session --format env)" 向け
```

| `--format` | 出力 |
|-----------|------|
| `tsv`（既定） | `session_id`, `session_dir`, `log_file`, `current_log` をタブ区切り行 |
| `json` | 上記フィールドの JSON オブジェクト |
| `env` | `AISH_SESSION_DIR`, `AISH_SESSION_ID`, `AISH_LOG_FILE`, `AISH_CURRENT_LOG` |

`AISH_SESSION_DIR` 未設定時はエラー。

## 手順（対話シェル）

1. 対話シェルを起動:

   ```bash
   cargo run -p aish -- shell
   ```

2. 表示された **session id**（12 桁 hex）を控える。
3. シェル内で `echo hello` と `exit` を実行する。
4. ログを確認:

   ```bash
   cat "$AISH_SESSION_DIR/log.jsonl"
   ```

## 手順（ai ask 連携）

### A: `aish shell` 内（推奨）

```bash
export AI_ASK_LOG=session
ai ask "hello の直前に何をしたか推測して"
```

または明示的に session id を渡す:

```bash
ai ask --session <12桁hex> "…"
```

（`<12桁hex>` は `basename "$AISH_SESSION_DIR"` と一致すること）

- `stderr` に `ai: using shell log: …/current_log` が出ること
- `AI_ASK_LOG` 未設定かつ `--session` なしではログを載せないこと

### B: 別ペイン

```bash
export AISH_SESSION_DIR=~/.local/share/aish/sessions/<12桁hex>
cargo run -p ai -- ask --session <12桁hex> "ログを要約して"
```

## 手順（aish exec — 従来）

```bash
cargo run -p aish -- exec --log /tmp/exec-test.jsonl -- echo hello
cat /tmp/exec-test.jsonl
```

## 期待結果

- ターミナルに `hello` が表示される（対話シェル）
- ログに `command_start`（`interactive_shell`）、`stdout`（`hello`）、`exit` が含まれる
- **`exit` 後、`aish shell` が即座に終了し親プロンプトに戻る**
- ログファイルのパーミッションが `600`
- `command_start` に API キー形式が平文で残らない（0012）

## CLI オプション（ai）

| オプション | 意味 |
|-----------|------|
| `--no-log` | ログなし（最優先） |
| `--log PATH` | 指定ファイル（`--session` より優先） |
| `--session ID` | `AISH_SESSION_DIR/current_log`（`ID` は **12 桁小文字 hex** で dir 名と一致） |

## よくある失敗

- `aish shell: --log is not supported` — `shell` では `--log` は使えない
- `--session requires AISH_SESSION_DIR` — `aish shell` の子シェルで実行するか、`AISH_SESSION_DIR` を export
- `--session <id> does not match AISH_SESSION_DIR` — id と `basename "$AISH_SESSION_DIR"` が一致していない
- `invalid session id` — `--session` は 12 桁の小文字 hex（`0-9a-f`）のみ
- `AISH_SESSION_DIR is not set` — `aish session` は `aish shell` の子シェル内、または `AISH_SESSION_DIR` を export した環境でのみ使える

**本手順は AI 未実施時は完了報告に「未実施」と明記する。**
