# 0002 — ai クライアントのツール連携 — 仕様ドラフト

> **出典**: Codex `spec`（2026-05-23）。レビュー反映（Cursor、2026-05-23）  
> **状態**: 実装済み（2026-05-23、MVP）。正本の要約は `../architecture.md` と本ファイル。本仕様は `ai` の `ask` におけるツール allowlist 解決と表示契約を定義する。

## 目的

`ai ask` が、ユーザーまたは設定で明示されたツール allowlist を `aibe` の `agent_turn` に渡し、`aibe` のツール付きエージェントループを利用できるようにする。

本仕様の狙いは、`ai` を LLM クライアントとして増やすことではなく、`aibe` のエージェント機能を安全に引き出すことにある。

## スコープ

### 対象

- `ai ask` の `tools` 指定
- `~/.config/ai/config.toml` の `[ask].tools`
- CLI `--tools`
- `ai` 側のツール名・カテゴリの検証
- `stdout` / `stderr` の表示契約
- `aibe` への送信前検証
- セキュリティ上の二重ゲート

### 対象外

- `list_tools` プロトコル
- 動的ツールディスカバリ
- マルチターン会話の CLI 状態管理
- `aibe` 設定の自動読み込み
- インタラクティブ shell 許可
- streaming
- `aibe` 側のツール実装そのもの

## 前提

- `ai` は `aibe` のクライアントであり、LLM HTTP API を直接呼ばない。
- `aibe` の組み込みツール名の正本は `aibe` から `pub` 露出される固定名である。
- 本仕様のツール名は `aibe` 側の公開名と一致する必要がある。
- `ai` は `aibe` の設定ファイルを読まない。
- `ai` が扱うツールは固定名とカテゴリエイリアスのみであり、UI ベースの列挙や動的補完は行わない。
- リクエストの `tools`（クライアント allowlist）と、aibe 設定による実行ポリシー（`enabled`, `allowed_commands`, `allowed_roots` 等）は **独立** である。サーバ側の拒否・無効時の挙動は `0001_aibe-tool-agent-loop-spec.md` に従う（turn 全体を即 `error` にせず、tool result で LLM に返してループ継続する場合がある）。

## 受け入れ条件

### 1. 既定互換

- `[ask].tools` のキー省略、または CLI / config とも未指定のときは `[]` として扱う。
- これは従来の `ai ask` 互換である。

### 2. 設定と CLI の優先順位

- `~/.config/ai/config.toml` の `[ask].tools` を既定の allowlist とする。
- CLI `--tools` が指定された場合は **その回だけ** 設定を上書きする（置換。マージしない）。
- 優先順位は `--tools` > `[ask].tools` > 省略時 `[]` である。
- config に `tools = "@read-only"` 等があっても、`--tools none` または `--tools @none` で **この回のみ** `[]` にできる。

### 3. 明示時のみ `shell_exec`

- `shell_exec` は明示された場合のみ有効にする（カテゴリ `@exec`、リテラル `shell_exec`、またはそれらを含む展開結果）。`@full` には含めない。
- `config` または CLI に書かれていない限り、`ai` が勝手に `shell_exec` を追加してはいけない。
- `@full` を config の既定にすることは推奨しない（例示用）。運用上の既定は `[]` またはユーザーが選んだ `@read-only` 等。

### 4. 未知名は socket 前に拒否

- 未知ツール名は `aibe` へ接続する前に `ai` が拒否する。
- 未知カテゴリも同様に、`aibe` へ接続する前に `ai` が拒否する。
- `ai` は deny 後に `aibe` を起動・接続しない。

### 5. 表示契約

- `stdout` には最終 `assistant_message.content` のみを出力する。
- `tool_calls` の詳細は `--verbose-tools` で `stderr` に出力する。
- `--verbose-tools` が無い通常経路では、`tool_calls` の詳細を `stdout` に出さない。
- `agent_turn_result.status` が `max_tool_rounds` のときも、最終 `assistant_message` を `stdout` に出す。あわせて `stderr` に警告 1 行を出す（下記 Presenter）。

### 6. 起動時の通知

- `ai` はツール解決後の実ツール名を **毎回** `stderr` に 1 行表示する（`[]` のときも `none` と表示）。
- 展開後の allowlist に `shell_exec` が含まれる場合、または指定トークンに `@exec` / リテラル `shell_exec` がある場合は `warning:` を付ける。

## CLI

### 形式

```bash
ai ask <message> [--log PATH] [--socket PATH] [--no-start] [--tools LIST] [--verbose-tools]
```

### `--tools LIST`

- **1 つの文字列**をカンマ区切りでトークン分割する（CLI 専用の構文）。
- 余白は解析時に trim する。
- `@` プレフィックス付きカテゴリと固定ツール名（`read_file`, `list_dir`, `grep`, `git_diff`, `git_status`, `shell_exec`）を混在できる。
- `--tools` は config より優先する。

#### `--tools none`（config 上書き）

- `--tools none` または `--tools @none` は、**この回のみ** allowlist を `[]` にする。
- `[ask].tools` に `@read-only` 等があっても、上記指定で無効化できる。
- `none` / `@none` と他トークンの併記（例: `--tools none,read_file`）はエラーとする。

#### 混在の例

```bash
ai ask "..." --tools @read-only,shell_exec
```

- 展開後は safe tools + `shell_exec`。`@full` とは別物。意図が読める場合に使う。

### `--verbose-tools`

- `tool_calls` の詳細を `stderr` に出す。
- 詳細には少なくともツール名、引数、成功・失敗、出力の要約を含める。
- `stdout` の最終応答は汚さない。
- 各 `output` は **切り詰める**。上限は aibe の `[tools] max_tool_output_bytes` と同程度のバイト数とし、超過分は省略または末尾に切り詰めた旨を示す（端末・ログへの漏洩抑制）。

## config

### 形式

`~/.config/ai/config.toml` に `[ask]` セクションを追加する。

```toml
socket_path = "~/.local/share/aibe/run.sock"

[ask]
# 例: 調査のみ。キー省略時の既定は []（ツールなし）。
tools = "@read-only"
# tools = ["@read-only"]
# tools = ["@read-only", "read_file"]
```

`../ai.config.example.toml` の `tools = "@read-only"` は **安全側の記載例** であり、仕様上の既定値ではない。

### `tools`

- 文字列でも配列でもよい。
- **文字列**: 1 要素の LIST として扱い、CLI と同様にカンマでトークン分割する（例: `"@read-only,shell_exec"`）。
- **配列**: 各要素を **1 トークン** として扱う。要素内のカンマは分割しない（例: `["@read-only", "read_file"]` は 2 トークン）。
- 文字列・配列のどちらでも、展開・検証のあと `Vec<String>`（ツール名）に正規化する。

## カテゴリ表

| 指定 | 展開後 |
|------|--------|
| `none` | `[]` |
| `@none` | `[]` |
| `@read-only` | `read_file`, `list_dir`, `grep`, `git_diff`, `git_status` |
| `@exec` | `shell_exec` |
| `@full` | `read_file`, `list_dir`, `grep`, `git_diff`, `git_status` |

### 補足

- `@full` の展開順は固定で `read_file`, `list_dir`, `grep`, `git_diff`, `git_status` とする。
- `none` / `@none` は単独指定のみ有効である。
- `none` / `@none` と他トークンの併記はエラーとする。
- カテゴリとリテラルの併記（例: `@read-only` + `shell_exec`）は **許可** する。展開後は safe tools + `shell_exec` となる。
- MVP では `@inspect` などの別名は増やさない。
- カテゴリ表は `ai` 側だけが知る。`aibe` はカテゴリを解釈しない。
- カテゴリ表と `aibe::KNOWN_TOOLS` の同期は `ai/tests/tool_catalog_sync.rs` で検証する。新ツール追加手順: `../manual/ai-ask-tools.md` §新規組み込みツール追加チェックリスト（`0009_ai-tool-category-sync-spec.md`）。

## 展開ルール

1. config の `[ask].tools` を読み込む（未設定ならトークン列は空）。
2. CLI `--tools` があれば、config のトークン列を **捨て**、CLI の LIST 文字列だけを使う。
3. 入力形式に応じてトークン列を得る:
   - **CLI** `--tools LIST`: LIST をカンマ分割し、各要素を trim。
   - **config 文字列**: 上記と同様にカンマ分割。
   - **config 配列**: 各配列要素を 1 トークンとして trim（要素内は分割しない）。
4. 各トークンについて:
   - `none` / `@none` → 単独なら `[]` で確定。他トークンと併記ならエラー。
   - `@` プレフィックス → カテゴリ表でツール名列に展開。
   - それ以外 → 固定ツール名として扱う。
5. 展開結果のツール名を **出現順を保ったまま重複除去** する（例: `@read-only` と `read_file` → `read_file` のみ）。
6. 最終結果を `Vec<String>` として `aibe` の `agent_turn.tools` に渡す。

### 検証

- 展開前に未知トークン（未知カテゴリ・未知ツール名）を弾く。
- 展開後に aibe 公開名と一致しない名前が残る場合も弾く。
- `none` / `@none` と他トークンの混在は弾く。
- エラーは `aibe` 接続前に返す。

## Presenter / 表示

- `stdout` は最終 `assistant_message.content` のみとする。
- `tool_calls` の詳細は `--verbose-tools` のときだけ `stderr` に流す（出力は「CLI — `--verbose-tools`」の切り詰めに従う）。
- 起動時の `stderr` 1 行は、解決後の実ツール名と、可能なら元の指定（カテゴリ）の要約とする。
- `stdout` にツールメタデータを混ぜない。

### `agent_turn_result.status`

| `status` | `stdout` | `stderr`（通常） | `stderr`（`--verbose-tools`） |
|----------|----------|------------------|-------------------------------|
| `ok` | 最終 assistant | 起動時 1 行のみ | 起動時 1 行 + `tool_calls` 詳細 |
| `max_tool_rounds` | 最終 assistant（部分結果） | 起動時 1 行 + 上限到達の warning 1 行 | 同上 + `tool_calls` 詳細 |

`max_tool_rounds` の warning 例:

```text
warning: ai: max tool rounds reached; showing partial assistant reply
```

### 起動時メッセージ例

```text
ai: tools enabled: read_file, list_dir, grep, git_diff, git_status (@read-only)
ai: tools enabled: read_file, list_dir, grep, git_diff, git_status (@full)
warning: ai: tools enabled: read_file, list_dir, grep, git_diff, git_status, shell_exec (@read-only,shell_exec)
warning: ai: tools enabled: shell_exec (@exec)
warning: ai: tools enabled: shell_exec (shell_exec)
ai: tools enabled: none
```

## セキュリティ

- **二重ゲート**: (1) 本仕様のクライアント allowlist（`ai` が `agent_turn.tools` に載せる名前）、(2) aibe サーバ設定（実行可否・コマンド allowlist・読取ルート）。`ai` は (2) を読まない。
- `ai` が `shell_exec` をリクエストしても、aibe で無効・allowlist 外のときは **0001** に従い、多くの場合 turn `error` ではなく tool result として LLM に返る。クライアントは `tool_calls` と `--verbose-tools` で観測する。
- `--tools` と config で `shell_exec` を明示しない限り、`ai` が `shell_exec` を allowlist に入れない。
- `--verbose-tools` は引数や tool output を `stderr` に出す。共有端末や記録されたログでは注意する（`../security.md`）。
- `stdout` が最終応答だけであることは、シェル連携での誤パース防止にもなる。

## テスト

| 種別 | 内容 |
|------|------|
| 単体 | トークン分割（CLI カンマ / config 配列）、カテゴリ展開、`none` 併記エラー、重複除去 |
| 単体 | 未知カテゴリ・未知ツール名で接続前エラー（モックで socket 未呼び出し） |
| 統合 | Mock / Scripted aibe で `--tools @read-only` → リクエストに `read_file`、`tool_calls` 非空の経路 |
| 統合 | config `tools = "@read-only"` + CLI `--tools none` → `tools: []` |
| 回帰 | `tools` 未指定の既存 `ai ask` 統合テスト |

手動検証手順: `../manual/ai-ask-tools.md`（A/B: mock・隔離設定、C: 実 LLM + `read_file`）。実施記録は同ファイル末尾（C はユーザー確認済み）。

## 影響クレート

| クレート | 変更 |
|---------|------|
| **ai** | `Ask`、config 読み込み、CLI 解析、Presenter、検証 |
| **aibe** | プロトコル変更なし。ツール名の `pub` 露出のみ（別コミット可） |
| **docs** | `architecture.md`, `security.md`, `ai.config.example.toml`, `AGENTS.md` |

レイヤー分割・cwd 必須・内部型強化は **`0003_architecture-review-refactor-spec.md`**（0002 実装後の追補）。

## 未確定・推測

| 種別 | 内容 |
|------|------|
| **推測** | `--verbose-tools` の行フォーマット（1 tool 1 行か JSON か）は実装時に微調整 |
| **推測** | 起動時メッセージの「元指定」表記（括弧内）の詳細 |
| **未確定** | `ai` 側での未知トークン・未知ツール名のエラー文言（例: `unknown tool category: foo`） |
| **未確定** | `aibe` 公開ツール名の取得方法（`pub` モジュールの具体的な API 形） |

## 残リスク

- `--verbose-tools` により、ツール引数や出力が端末ログへ露出する可能性がある。
- クライアント側 allowlist は実行抑止であり、`aibe` 側の権限制御そのものを代替しない。
- 固定ツール名が増えた場合、`ai` 側のカテゴリ表との同期漏れが起こりうる。
