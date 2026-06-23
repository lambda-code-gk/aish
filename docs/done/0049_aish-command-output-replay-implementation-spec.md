# 0049 — `aish` command output replay 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計の正本**: [0049_aish-command-output-replay-spec.md](../spec/0049_aish-command-output-replay-spec.md)  
> **状態**: 実装指示書  
> **起票**: 2026-06-23  
> **関連**: [architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[aish-shell-log.md](../manual/aish-shell-log.md)、[`scripts/spec-acceptance.toml`](../../scripts/spec-acceptance.toml)、[`docs/0000_spec-index.md`](../0000_spec-index.md)

## 0. 目的

`docs/spec/0049_aish-command-output-replay-spec.md` を満たすために、`aish` クレート内へ command span 記録と replay CLI を追加する。  
`aish shell` の bash / zsh 対話入力も replay 対象に含め、`aish replay list/show/pick` で過去の出力を **再実行せず** に取り出せるようにする。  
実装は `aish` クレートに閉じ、`ai` / `aibe` / wire protocol は触らない。

## 1. パック構成の適用

**No**。この変更は optional 機能束の脱着ではなく、`aish` の core である「シェル実行 + ログ記録 + ログ再表示」の拡張である。Active Pack / Basic Pack を導入する対象ではない。

## 2. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | `LogEvent` の span 拡張、`exec` / `shell` の command span 記録、bash / zsh の control channel 配線を実装する。旧 JSONL を壊さない serde 後方互換と、partial span を replay 対象外にする判定を先に固定する。 | Phase 1 の AC がすべて `pending = false` になるまで Phase 2 に進まない |
| 2 | `aish replay list/show` を追加し、`--log PATH` / `AISH_SESSION_DIR` / `--index` / `--stderr` を配線する。`current_log` の安全な解決もこの Phase で固める。 | Phase 2 の AC がすべて `pending = false` になるまで Phase 3 に進まない |
| 3 | `aish replay pick` を追加し、`fzf` 優先 + 内蔵 fallback + non-TTY fail-closed を実装する。manual と docs を仕上げる。 | 全 AC が `pending = false` で `./scripts/verify.sh` が通る |

## 3. 変更ファイル一覧

### 3.1 `aish` 実装

| パス | 役割 |
|------|------|
| `aish/src/domain/log_event.rs` | `LogEvent` に `command_index` / `started_at` / `finished_at` / `exit_code` を載せる。旧ログの serde 後方互換を保つ。span 付き生成関数を追加する。 |
| `aish/src/adapters/inbound/clap_cli.rs` | `replay` サブコマンドと `list/show/pick` の clap 定義を追加する。`--log` / `--index` / `--stderr` / list 用 format を定義する。 |
| `aish/src/main.rs` | `AishCommand::Replay` の分岐を追加し、`run_replay_list` / `run_replay_show` / `run_replay_pick` へ配線する。 |
| `aish/src/application/replay.rs` | replay の core をまとめる新規 application。log 解決、span grouping、list/show の整形、picker の orchestration を置く。 |
| `aish/src/adapters/outbound/session_info.rs` | `AISH_SESSION_DIR/current_log` の canonicalize / regular-file / symlink escape 検証を追加する。 |
| `aish/src/adapters/outbound/shell_completion.rs` | bash / zsh の一時 rcfile に control channel hook を注入する。`AISH_COMMAND_OUTPUT_FD` などの inherited FD 参照をここで組み立てる。 |
| `aish/src/adapters/outbound/pty_shell.rs` | control pipe の作成、child への FD 継承、parent 側の control message 受信、command span 採番、PTY 出力との統合を行う。 |
| `aish/src/adapters/outbound/replay_picker.rs` | `fzf` 呼び出しと内蔵 fallback を担当する新規 outbound adapter。TTY 要件もここか application で判定する。 |
| `aish/src/adapters/inbound/mod.rs` / `aish/src/adapters/outbound/mod.rs` / `aish/src/application/mod.rs` | 新モジュール公開を追加する。 |
| `aish/tests/exec_log.rs` | `exec` の span 記録と serde 互換の回帰を固定する。 |
| `aish/tests/session_cli.rs` | `AISH_SESSION_DIR` 未設定・既存 session との整合の回帰を追加する。 |
| `aish/tests/replay_cli.rs` | `replay list/show/pick` の CLI 契約、TTY / non-TTY、`--stderr` 制約を固定する。 |

### 3.2 docs

| パス | 役割 |
|------|------|
| `docs/architecture.md` | `aish` の event model に command span / command_index / replay CLI を追記する。 |
| `docs/security.md` | replay が redacted ログしか読まないこと、`current_log` の escape 拒否、0600 前提を追記する。 |
| `docs/testing.md` | `aish` replay の unit / integration / manual 配置を追記する。 |
| `docs/manual/aish-shell-log.md` | 既存の shell log 手順に replay 導線を追記する。 |
| `docs/manual/aish-command-output-replay.md` | 新規 manual。`list/show/pick` の確認手順を分離して書く。 |
| `docs/manual/README.md` | 新 manual の一覧追加。 |
| `docs/0000_spec-index.md` | `docs/tasks/` に 0049 を追加する。 |

## 4. 実装手順

### 4.1 Phase 1: `LogEvent` / `exec` / shell hook / control channel

#### 4.1.1 `LogEvent` を span 対応へ拡張する

対象:

- `aish/src/domain/log_event.rs`

作業内容:

1. 既存の event 名 (`command_start` / `stdout` / `stderr` / `exit`) は維持する。
2. 新規フィールドは `Option` + `#[serde(default)]` + `skip_serializing_if = "Option::is_none"` で追加し、旧 JSONL をそのまま読めるようにする。
3. 旧ログからの deserialize では、span 情報が欠けていても失敗させない。
4. replay に必要な情報は新規ログにのみ付与する。古いログは replay 対象外に落とす。
5. `LogEvent::command_start` だけでなく、span 付きの生成関数を追加する。
   - `command_index` を持つ start / stdout / stderr / end
   - `started_at` / `finished_at`
   - `exit_code`
6. `command_index` が `None` のイベントは「span 未確定」として扱い、grouping から除外できるようにする。
7. `#[cfg(test)]` に旧 JSONL の deserialize 回帰と、span 付き roundtrip の両方を置く。

#### 4.1.2 `aish exec` に command span を付ける

対象:

- `aish/src/application/execute_and_record.rs`
- `aish/src/main.rs`

作業内容:

1. `ExecuteAndRecord::run` の先頭で `command_index = 1` の span を開始する。
2. `started_at` は `shell.run` の直前に親側で採る。
3. `Stdout` / `Stderr` は `command_index = 1` を持つイベントとして記録する。
4. `Exit` は command span の終端として扱い、`finished_at` と `exit_code` を付ける。
5. replay 用の出力はログに残った redacted テキストをそのまま使う。`sanitize_log_text` の再適用はしない。
6. `exec_log.rs` には、`aish exec --log ... -- echo ...` の結果から span metadata が入っていることを確認する integration 回帰を追加する。

#### 4.1.3 bash / zsh hook で control channel を渡す

対象:

- `aish/src/adapters/outbound/shell_completion.rs`
- `aish/src/adapters/outbound/pty_shell.rs`

作業内容:

1. `prepare_interactive_rc` の戻り値に、control channel 用の設定を追加できるようにする。
2. `write_bash_wrapper` / `write_zsh_wrapper` で、既存 completion snippet に加えて replay hook を注入する。
3. hook は `AISH_COMMAND_OUTPUT_FD` のような環境変数から inherited FD を参照し、1 行 1 JSON の `start` / `end` をそこへだけ書く。
4. PTY の stdout / stderr は control message とみなさない。control channel は独立した pipe のみを使う。
5. shell hook は時刻文字列を組み立てない。親側が marker 受信時に時刻を採る。
6. hook 注入に失敗した shell は replay 対象外として扱う。shell 自体は従来どおり起動し、list/pick からは除外する。
7. `ChildShellKind::Other` は v1 では replay 境界を持たなくてよい。

#### 4.1.4 parent 側で control pipe を読む

対象:

- `aish/src/adapters/outbound/pty_shell.rs`

作業内容:

1. `run_shell_parent` かその直下に control pipe を追加する。
2. child へは write 側だけを継承させ、parent は read 側だけを持つ。
3. `child_exec_shell` で write FD を環境変数に載せるか、固定 FD 番号を渡す。
4. `relay_master_fd` で PTY master と control pipe を同時に poll する。
5. control message の JSON が malformed なら破棄し、その span は replay 不可にする。
6. `start` / `end` が揃わない、途中で切れた、順序が壊れた span は partial として除外する。
7. `relay_master_chunk` / `flush_line` は PTY visible output の記録に閉じ込め、control message と混ぜない。
8. replay 対象の shell span には `command_index` を振り、stdout の行にも同じ index を付ける。

### 4.2 Phase 2: `aish replay list/show`

#### 4.2.1 `clap` 定義を追加する

対象:

- `aish/src/adapters/inbound/clap_cli.rs`
- `aish/src/main.rs`

作業内容:

1. `AishCommand` に `Replay` サブコマンドを追加する。
2. `ReplayCommand` を nested subcommand として定義し、`List` / `Show` / `Pick` に分ける。
3. `List` は `--log PATH` / `--index N` / `--format tsv|json` を受ける。
4. `List` の `--format` では `env` を受けない。`tsv` と `json` のみ許可する。
5. `Show` は `--log PATH` / `--index N` / `--stderr` を受ける。
6. `Pick` は `--log PATH` / `--index N` / `--stderr` を受ける。
7. `main.rs` は `AishCommand::Replay` を `run_replay_list` / `run_replay_show` / `run_replay_pick` へ配線する。

#### 4.2.2 replay core を application に寄せる

対象:

- `aish/src/application/replay.rs`
- `aish/src/application/mod.rs`

作業内容:

1. log source 解決を 1 箇所にまとめる。
   - `--log PATH`
   - `AISH_SESSION_DIR/current_log`
   - session dir 外への escape 拒否
2. JSONL を読み、`command_index` ごとに span を group する。
3. list は complete span のみを列挙し、partial / malformed / incomplete span は出さない。
4. show は `index` 指定の span を返し、stdout 既定・stderr 明示の契約を守る。
5. shell span では `--stderr` をエラーにする。
6. `show` / `list` ともに、余計なヘッダや注釈を stdout に出さない。`| rg` が壊れないことを優先する。
7. replay は既存の redacted log をそのまま流す。再 sanitize しない。

#### 4.2.3 `current_log` の安全な解決を固める

対象:

- `aish/src/adapters/outbound/session_info.rs`
- 必要なら `aish/src/application/replay.rs`

作業内容:

1. `AISH_SESSION_DIR` があるときは `current_log` を既定解決する。
2. canonicalize 後の実体が session dir 配下にあることを確認する。
3. 通常ファイルであることを確認する。
4. symlink escape を拒否する。
5. `--log PATH` は明示ユーザー入力として扱うが、少なくとも通常ファイルかつ read-only であることは確認する。

#### 4.2.4 integration tests を追加する

対象:

- `aish/tests/replay_cli.rs`

作業内容:

1. `replay list` が complete span を一覧できることを確認する。
2. `replay show --index N` が再実行なしで stdout を返すことを確認する。
3. `replay show --stderr` が exec span の stderr のみを出せることを確認する。
4. `replay show --stderr` が shell span では失敗することを確認する。
5. `AISH_SESSION_DIR` ありで `current_log` が既定解決されることを確認する。
6. `--log PATH` が `AISH_SESSION_DIR` なしでも動くことを確認する。

### 4.3 Phase 3: `aish replay pick`

#### 4.3.1 picker を実装する

対象:

- `aish/src/adapters/outbound/replay_picker.rs`
- `aish/src/application/replay.rs`

作業内容:

1. `pick` は stdin / stdout / stderr が TTY でなければ fail-closed にする。
2. `fzf` が PATH にあれば優先的に使う。
3. `fzf` がない場合は、内蔵の簡易セレクタに fallback する。
4. fallback では index / started_at / exit_code / kind / command preview を表示する。
5. 選択後は `show` と同じ stdout 契約で replay する。
6. `show --stderr` と組み合わせた場合の shell span / exec span の制約を守る。
7. non-TTY の場合は `list + show --index` を案内し、無理に対話化しない。

#### 4.3.2 picker のテストを追加する

対象:

- `aish/tests/replay_cli.rs`
- もしくは `aish/src/adapters/outbound/replay_picker.rs`

作業内容:

1. `fzf` があるときに優先されることを固定する。
2. `fzf` がないときに fallback が動くことを固定する。
3. non-TTY では fail-closed になることを固定する。
4. それぞれ `#[ignore]` 付きで先に置き、実装後に外す。

## 5. 受け入れ条件

設計書 §11 を、このクレートのテスト関数に落とす。

| ID | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| AC-01 | `LogEvent` の旧 JSONL を serde で読める | `log_event_serde_is_backward_compatible` | true |
| AC-02 | `exec` のログに span metadata が入る | `exec_command_span_records_index_and_timestamps` | true |
| AC-03 | shell hook / control channel で command span を記録できる | `shell_command_span_records_index_and_timestamps` | true |
| AC-04 | `AISH_SESSION_DIR/current_log` の安全な解決ができる | `replay_current_log_resolution_rejects_escape` | true |
| AC-05 | `replay list` が complete span だけを出す | `replay_list_shows_only_complete_spans` | true |
| AC-06 | `replay show --index N` が stdout / stderr 契約を守る | `replay_show_emits_recorded_streams_without_resanitizing` | true |
| AC-07 | `replay show --stderr` が shell span では失敗する | `replay_show_rejects_shell_stderr` | true |
| AC-08 | `replay pick` が fzf 優先で動く | `replay_pick_prefers_fzf_when_available` | true |
| AC-09 | `replay pick` が fzf なしで fallback する | `replay_pick_falls_back_to_builtin_selector` | true |
| AC-10 | `replay pick` が non-TTY で fail-closed する | `replay_pick_rejects_non_tty` | true |

## 6. `scripts/spec-acceptance.toml` 登録案

`spec = "0049"` として 10 件を追加し、初期値は **すべて `pending = true`** とする。  
Phase 1 の AC は `#[ignore]` 付きで先に追加し、実装後に `pending = false` へ切り替える。

| Phase | id | description | test | file_glob | pending |
|------|----|-------------|------|-----------|---------|
| 1 | `log_event_serde_backward_compat` | 旧 JSONL の deserialize と span roundtrip を保証する | `log_event_serde_is_backward_compatible` | `aish/src/domain/log_event.rs` | true |
| 1 | `exec_span_metadata` | `aish exec` が command span metadata を記録する | `exec_command_span_records_index_and_timestamps` | `aish/tests/exec_log.rs` | true |
| 1 | `shell_span_metadata` | shell hook / control channel が command span を記録する | `shell_command_span_records_index_and_timestamps` | `aish/src/adapters/outbound/pty_shell.rs` | true |
| 2 | `current_log_escape_guard` | `AISH_SESSION_DIR/current_log` の escape を拒否する | `replay_current_log_resolution_rejects_escape` | `aish/src/adapters/outbound/session_info.rs` | true |
| 2 | `list_complete_spans` | `replay list` が complete span のみを一覧する | `replay_list_shows_only_complete_spans` | `aish/src/application/replay.rs` | true |
| 2 | `show_stream_contract` | `replay show` が recorded stdout/stderr をそのまま出す | `replay_show_emits_recorded_streams_without_resanitizing` | `aish/tests/replay_cli.rs` | true |
| 2 | `show_shell_stderr_error` | shell span に対する `--stderr` は失敗する | `replay_show_rejects_shell_stderr` | `aish/tests/replay_cli.rs` | true |
| 3 | `pick_fzf_preferred` | fzf があれば優先的に使う | `replay_pick_prefers_fzf_when_available` | `aish/src/adapters/outbound/replay_picker.rs` | true |
| 3 | `pick_builtin_fallback` | fzf が無くても内蔵 fallback で選べる | `replay_pick_falls_back_to_builtin_selector` | `aish/src/adapters/outbound/replay_picker.rs` | true |
| 3 | `pick_non_tty_fail_closed` | non-TTY では fail-closed になる | `replay_pick_rejects_non_tty` | `aish/tests/replay_cli.rs` | true |

## 7. docs 更新対象

同じ変更で次を更新する。

1. `docs/architecture.md`
   - `aish` の event model に command span / command_index を追記する。
   - `replay list/show/pick` の stdout 契約を追記する。
2. `docs/security.md`
   - replay が redacted ログのみを読むことを明記する。
   - `AISH_SESSION_DIR/current_log` の symlink escape 拒否を追記する。
   - 0600 前提を維持することを明記する。
3. `docs/testing.md`
   - `aish` replay の unit / integration / manual の置き場所を追加する。
4. `docs/manual/aish-shell-log.md`
   - 既存の shell log 手順から replay 導線へ進めるよう追記する。
5. `docs/manual/aish-command-output-replay.md`
   - 新規作成。`list` / `show` / `pick` を分けて手順化する。
6. `docs/manual/README.md`
   - 新 manual を一覧に追加する。
7. `docs/0000_spec-index.md`
   - `docs/tasks/` の 0049 を進行中として追加する。

## 8. 手動検証手順

新規 manual `docs/manual/aish-command-output-replay.md` に次を記載する。

1. `aish shell` を TTY で起動し、`echo hello` と `exit` を実行する。
2. `aish exec` で `stdout` / `stderr` が記録されることを確認する。
3. `aish replay list` で command span が一覧されることを確認する。
4. `aish replay show --index N` で再実行なしに stdout が出ることを確認する。
5. `aish replay show --index N | rg ...` が成立することを確認する。
6. `aish replay show --stderr --index N` が exec span でのみ使えることを確認する。
7. `aish replay pick` を TTY で実行し、`fzf` があれば優先されること、なければ内蔵 fallback が使えることを確認する。
8. `AISH_SESSION_DIR` を使った `current_log` 既定解決と、symlink escape 拒否を確認する。

## 9. smoke-mock 由来の導通確認コマンド

`aish` は LLM や `aibe` に依存しないため、`smoke-mock` 相当の確認は temp HOME + pseudo-TTY で再現する。  
manual には次のような導通確認コマンドを載せる。

```bash
tmp_home="$(mktemp -d)"
tmp_log_dir="$tmp_home/.local/share/aish/sessions"
mkdir -p "$tmp_log_dir"

script -qfec "HOME='$tmp_home' cargo run -p aish -- shell" /dev/null

# shell 内で:
#   echo hello
#   exit
#
# 退出後:
session_dir="$(find \"$tmp_log_dir\" -mindepth 1 -maxdepth 1 -type d | sort | tail -n 1)"
HOME="$tmp_home" AISH_SESSION_DIR="$session_dir" cargo run -p aish -- replay list
HOME="$tmp_home" AISH_SESSION_DIR="$session_dir" cargo run -p aish -- replay show --index 1
```

`aish shell` の対話部分は実際の TTY で行う。自動化する場合は、`script` か同等の pty ハーネスを使う。

## 10. 完了条件

1. 全 AC の `pending` が `false` になる。
2. `./scripts/verify.sh` が通る。
3. `docs/` 更新が同じ変更に含まれる。
4. 本ファイルを `docs/done/` へ移動し、`docs/0000_spec-index.md` を「実装済み」へ更新する。

## 11. 仕様との差分

- なし
