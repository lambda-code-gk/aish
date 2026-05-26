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

### ai のツール表示

- `ai ask` の `stdout` は最終 `assistant_message.content` のみに保つ。
- `--verbose-tools` で出す `tool_calls` 詳細は `stderr` に限定する。
- `tool_calls` の引数や出力には秘密情報が含まれうるため、共有端末や記録されたログでは注意する。
- `stderr` の通知文は補助情報であり、ツール実行の監査ログの代替ではない。

## 権限・プロセス

- **Unix 専用**: ファイルモード・ソケットパスは umask / `chmod` を意識する
- aibe の socket: ユーザー私用ディレクトリ配下。`bind` 時は umask `077` + `chmod 600`（foreground / デーモン共通）
- aish が起動するシェルは **ユーザー自身の権限** で動く。エージェントのツール実行はその権限を継承する — 高権限シェルでは aibe を動かさない運用を推奨
- **`shell_exec` タイムアウト**: aibe は timeout 時に子プロセスへ `start_kill()` を送り、**明示 `wait()` で reap** する。`kill_on_drop(true)` は補助的な保険であり、明示 kill/reap の代替ではない

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
