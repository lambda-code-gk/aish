# セキュリティ

優先順位 1 の要件。実装・設定・ログの変更時はこの文書と `docs/architecture.md` を見直す。

## 脅威モデル（簡易）

| 脅威 | 影響 | 主な対策 |
|------|------|----------|
| API キー漏洩 | 課金・データ流出 | キーは aibe 設定のみ、ログ・git 禁止 |
| シェルログ漏洩 | コマンド履歴・パスワード露出 | マスク方針、ファイル権限、コンテキスト最小化 |
| ai が LLM を直叩き | キー分散・監査不能 | 依存チェック + コードレビュー |
| aish がネットワーク | 想定外のデータ送信 | aish に HTTP クライアントを入れない |
| デーモン socket | ローカル任意コード実行相当 | socket パス・パーミッション |
| 外部コマンド（CLI coding agent 含む） | ユーザー権限で任意コマンド・ファイル変更 | 設定の `command` のみ起動、`cwd` は `context.cwd`、allowlist、承認、監査、自動 git なし |

### CLI サブエージェント（0024 / 0025, 非採用）

- API キーは aibe 設定に置かず、**ホストにインストール済みの Codex / Claude Code CLI** のログイン契約に委ねる、という考え方自体は 0024 / 0025 の旧案として残す。
- ただし本番採用はしない。`artifacts` や thread 共有は AISH の正本ではない。
- CLI を first-class provider / tool にしないため、`command --version` のような存在確認を aibe の起動要件にはしない。

### 外部コマンド（0026）

- `[[external_commands]]` は `shell_exec` のテンプレートであり、AISH のポリシー外の CLI を明示登録するためのもの。
- `risk_class` は `DangerousShell` のまま扱う。`approval_state` は既存の `shell_exec_approval` に従う。
- `approval_source` は少なくとも `shell_exec_approval=<mode>` を残し、必要ならプリセット名も追えるようにする。
- AISH は CLI の内部 sandbox / login / tool catalog / thread を検証しない。ユーザー責任で管理する。

## 秘密情報

### 置いてよい場所

- **aibe 設定ファイル**（例: `~/.config/aibe/config.toml`）の API キー
- OS の環境変数（aibe 起動プロセスのみが読む。`ai` / `aish` からは読まない設計）

### 置いてはいけない場所

- Git リポジトリ（`.env`, `config.toml` 実体）
- `aish` ログ（平文キー、Bearer トークン）
- `ai` の設定・バイナリ埋め込み
- ソースコード内の文字列リテラル

### 例示ファイル

- `*.example.toml` や `docs/` には **プレースホルダ** のみ（`YOUR_API_KEY` 等）

## ログとコンテキスト

### aish ログ

- 記録してよい: コマンド行、stdout/stderr、終了コード、時刻、作業ディレクトリ
- 記録しない / マスクする:
  - 環境変数の値（特に `*KEY*`, `*TOKEN*`, `*SECRET*`)
  - パスワードプロンプト直後の入力
  - `.env` ファイルの内容をそのまま

### 実装済みマスク（aish）

`aish` はログ追記前に `sanitize_log_text` を通す（`aish/src/domain/sanitize.rs`）。

| 適用先 | タイミング |
|--------|------------|
| `command_start` の `command` / 各 `args` | `LogEvent::command_start` 生成時（`exec` / `shell` 共通） |
| `stdout` / `stderr` の `data` | `ExecuteAndRecord` / PTY 追記時 |

| パターン | 置換 |
|---------|------|
| `sk-...`（OpenAI 形式） | `sk-[REDACTED]` |
| `Bearer ...` | `Bearer [REDACTED]` |
| `AIza...`（Google API キー形式） | `AIza[REDACTED]` |
| 環境変数名に `KEY` / `TOKEN` / `SECRET` を含む `NAME=value` | `NAME=[REDACTED]` |

`command_start` は `LogEvent::command_start` 経由でのみ安全化する。`CommandStart` の enum 直構築は使わない。

完全な秘匿ではない。パスワードプロンプト直後の入力などは今後拡張する。

### ai → aibe の context

- 渡すのは **必要最小限** のログ tail（上限は `aibe::ShellLogTail::MAX_BYTES`。ai はこの定数のみ参照しリテラル直書きしない）
- `cwd` はツール有効時に `ai` のカレントディレクトリ（絶対パス）を送り、`read_file` / `shell_exec` の相対パス解決に使う。**未送信・相対パスは aibe が `invalid_request` で拒否する**（aibe プロセス cwd へのフォールバックはしない）
- ユーザーが明示しない限り、全セッション履歴を一度に送らない設計を推奨

### ai local history

- `ai history` の `index.jsonl` には raw message / raw shell log tail / 秘密情報を載せない
- replay 用 payload vault は `payloads/<history_id>.json` に分離し、0600 相当の権限で保存する
- `history_id` から復元できない情報は index に置かず、再送に必要な最小限だけを vault に閉じ込める

### safe tools / dangerous tools

設計の上位正本: [architecture.md](architecture.md)。検証の所在: [testing.md](testing.md) の「0018 safe-tools-policy」。

| 区分 | ツール例 | 許可の考え方 |
|------|----------|--------------|
| **safe** | `read_file`, `list_dir`, `grep`, `git_diff`, `git_status` | `@read-only` / `@full` で既定有効。読み取り目的で `shell_exec` を使わない |
| **dangerous** | `shell_exec` | `@exec` または literal 指定が **明示** された場合のみ。それ以外は aibe が拒否する。実行前承認は aibe 設定 `[tools.shell_exec] shell_exec_approval`（`never` / `ask` / `always`、既定 `ask`）。`ask` は同一 Unix 接続上で `command` / `args` を表示して yes/no（非対話 stdin は fail-closed） |
| **将来（未実装）** | `write_file`, `replace_file`, `apply_patch` | 導入時は dry-run → approval → execute の順を前提にする（現リポジトリに当該ツールはない） |

- **client 側（`ai`）**: 起動時に有効ツールを `stderr` で表示。`shell_exec` 有効時は warning。`shell_exec_approval=ask` では実行直前 yes/no も **stderr**（`Execute? [y/N]`）。stdin が TTY でない場合は **読む前に拒否**（`stdin.is_terminal()`）。`command` / `args` は `escape_default` 相当で表示し、ANSI / 制御文字の見た目偽装を防ぐ（0023）。
- **server 側（`aibe`）**: allowlist 外・`shell_exec_approval=never`・ユーザー拒否は tool result（`status=error`）で LLM に返し turn 継続可能。client warning の有無に依存しない。
- **監査**: `tool_calls` に `risk_class` / `approval_state` / `approval_source`（例: `shell_exec_approval=ask`）/ `decision` を載せる（0020）。
- manual や logs に残す説明は、本番設定や秘密情報を含めない。

### ai のツール表示

- `ai ask` の `stdout` は最終 `assistant_message.content` のみに保つ。
- `--verbose-tools` で出す `tool_calls` 詳細は `stderr` に限定する。
- `tool_calls` の引数や出力には秘密情報が含まれうるため、共有端末や記録されたログでは注意する。
- `stderr` の通知文は補助情報であり、ツール実行の監査ログの代替ではない。

### ai の output filter（`AI_FILTER` / `[ask].filter`）

- filter は **ユーザー端末上**で `/bin/sh -c` として実行される。assistant 本文が stdin に渡る。
- filter 文字列はユーザー自身が設定する。**任意のシェルコマンド**として扱い、設定ファイルや env に信頼できない第三者の入力を置かない。
- filter の stdout はターミナルにそのまま出る。機密を含む応答を加工する場合も、シェル履歴・ログへの露出に注意する。
- filter stderr はユーザー stderr に透過する。filter がハングすると `ai ask` も終了しない（タイムアウトなし）。
- `aish` は filter を export しない。`aish shell` 内で使う場合はユーザーが明示的に `export AI_FILTER=...` する。

### ai local history

- `index.jsonl` は redacted メタデータのみを置く。raw message、raw shell log tail、秘密情報を入れない
- `payloads/<history_id>.json` は replay 用 payload vault。`0600` 相当で保存し、他ユーザーから読めない前提を維持する
- `retry` / `rerun` の実行時に payload の生値を stderr に再掲しない

## 権限・プロセス

- **Unix 専用**: ファイルモード・ソケットパスは umask / `chmod` を意識する
- aibe の socket: ユーザー私用ディレクトリ配下。`bind` 時は umask `077` + `chmod 600`（foreground / デーモン共通）
- aish が起動するシェルは **ユーザー自身の権限** で動く。エージェントのツール実行はその権限を継承する — 高権限シェルでは aibe を動かさない運用を推奨
- **外部コマンドのタイムアウト**（`shell_exec` / `git_diff` / `git_status` / `git rev-parse`）: aibe は timeout 時に子プロセスへ `start_kill()` を送り、**明示 `wait()` で reap** する（共通 `run_subprocess`）。`kill_on_drop(true)` は補助的な保険であり、明示 kill/reap の代替ではない
- **`list_dir` / `grep`**: `[tools.explore]` の件数・走査上限で timeout 前のメモリ・I/O を抑制する（`docs/aibe.config.example.toml`）

## 依存関係

- `./scripts/check-architecture.sh` で禁止クレート依存を検出
- 新規 crate 追加時は LLM/HTTP 系が **aibe 以外** に入っていないか確認

## インシデント時

1. 漏洩疑いのキーはプロバイダ側で **即ローテーション**
2. ログファイル・設定のバックアップを確認し、共有範囲を切る
3. 再発防止を `docs/security.md` に追記

## レビューチェックリスト（変更時）

- [ ] 秘密が git / ログ / テスト fixture に入っていないか
- [ ] aish にネットワーク依存が増えていないか
- [ ] ai が LLM URL に直接つながないか
- [ ] 手動検証手順に本番キーを書いていないか
