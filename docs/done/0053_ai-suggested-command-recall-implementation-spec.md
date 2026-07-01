# 0053 — `ai` 提案コマンド再呼び出し 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計の正本**: [0053_ai-suggested-command-recall-spec.md](../spec/0053_ai-suggested-command-recall-spec.md)  
> **状態**: 実装指示書  
> **起票**: 2026-07-01  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[security.md](../security.md)、[manual/ai-ux.md](../manual/ai-ux.md)、[`scripts/spec-acceptance.toml`](../../scripts/spec-acceptance.toml)、[`docs/0000_spec-index.md`](../0000_spec-index.md)

## 0. 目的

`docs/spec/0053_ai-suggested-command-recall-spec.md` を満たすために、`ai` が assistant の final message から shell command 候補を抽出して session-scoped cache に保存し、`aish shell` と `ai complete bash|zsh` の両方から同じ recall hook を配線する本番経路を追加する。  
`replay` 系とは別機能として、再実行ではなく prompt buffer への挿入に限定し、`bash` / `zsh` 以外は fail-closed にする。`aibe` は変更せず、候補保存・hint・hook 生成は `ai` と `aish` の責務に閉じる。

## 1. パック構成の適用

**部分適用**。新しい Pack は作らず、`ai` の recall 抽出・cache 保存と `aish` の shell bootstrap を既存 boundary のままつなぐ。

理由は 2 点ある。

1. 本機能は optional な shell UX だが、`aibe` / `aish` を pack として差し替える話ではない。`ai` が候補を集め、`aish` が shell hook を配るという責務分離で十分である。
2. recall cache は `ai` のローカル状態、hook 配線は `aish` の rcfile 注入であり、composition root を新設するほどの共有境界はない。

## 2. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | `ai` の domain / port / adapter を追加して、final assistant content からの候補抽出、正規化、queue 保持、session-scoped cache 保存、TTY / quiet / format / non-TTY 判定、stderr hint を実装する。`scripts/spec-acceptance.toml` と unit / integration テストを先に固める。 | Phase 1 の AC がすべて `pending = false` になるまで Phase 2 に進まない |
| 2 | `aish shell` の rcfile 注入と `ai complete bash|zsh` の出力を同一 hook に揃え、`Alt+.` の binding、bash / zsh の buffer 挿入、unsupported shell の fail-closed、docs / index 同期までを実施する。 | Phase 2 の AC がすべて `pending = false` になるまで完了扱いにしない |

## 3. 変更ファイル一覧

### 3.1 Phase 1

| パス | 役割 |
|------|------|
| `ai/src/domain/suggested_command_recall.rs`（新規） | fenced code block 抽出、language tag 判定、prompt prefix の正規化、NUL / 制御文字 / oversize の拒否、queue モデルの正本を置く。 |
| `ai/src/ports/outbound/suggested_command_recall_store.rs`（新規） | session-scoped cache の保存 / 読み出し port。atomic replace と read-only load の境界を定義する。 |
| `ai/src/application/suggested_command_recall.rs`（新規） | turn 終了時の recall flow、TTY / quiet / format / non-TTY 判定、stderr hint の組み立てをまとめる。 |
| `ai/src/adapters/outbound/suggested_command_recall_store.rs`（新規） | JSON cache の実ファイル操作、0600、tmp file + rename、壊れた cache の fail-closed を担う。 |
| `ai/src/adapters/outbound/toml_config.rs` | `[ask].suggested_command_recall*` の設定読み込みと default 解決を追加する。 |
| `ai/src/adapters/outbound/mod.rs` | recall store / helper の re-export を追加する。 |
| `ai/src/main.rs` | turn の終端で recall 保存と hint 表示を呼ぶ wiring を追加する。 |
| `ai/tests/suggested_command_recall.rs`（新規） | 抽出、queue 順、cache save/load、TTY / quiet / format / non-TTY の回帰を固定する。 |
| `scripts/spec-acceptance.toml` | 0053 の AC を 1:1 で登録する。 |

### 3.2 Phase 2

| パス | 役割 |
|------|------|
| `aish/src/adapters/outbound/shell_completion.rs` | `aish shell` の rcfile に recall hook を追加し、`ai complete` 由来の hook ブロックを一貫して注入する。 |
| `aish/src/adapters/outbound/mod.rs` | recall hook 生成 helper の re-export を追加する。 |
| `ai/src/clap_cli.rs` | `ai complete bash|zsh` の出力末尾に recall hook trailer を追加する。 |
| `ai/src/adapters/outbound/shell_completion.rs`（新規） | `ai complete` と `aish shell` が共有する bash / zsh hook 文字列の正本を置く。 |
| `ai/src/adapters/outbound/mod.rs` | recall hook 生成 helper の re-export を追加する。 |
| `ai/src/main.rs` | `complete` 実行時の hook trailer 配線と、unsupported shell での no-op を結ぶ。 |
| `docs/architecture.md` | shell completion / recall の責務分離、cache ownership、Alt+. の role を追記する。 |
| `docs/manual/ai-ux.md` | recall の手動確認手順を追記する。 |
| `docs/0000_spec-index.md` | `tasks/` の 0053 を追加する。 |

## 4. 実装手順

### 4.1 domain

1. `fenced code block` を走査する extractor を domain に置く。
2. 受理する language tag を `bash` / `sh` / `zsh` / `shell` に限定する。
3. 先頭・末尾の空行、uniform prompt prefix、trailing newline の正規化を実装する。
4. NUL、制御文字、ANSI escape、8 KiB 超過を明示的に reject する。
5. 1 turn から得た複数候補を順序付き queue として保持する。

### 4.2 ports

1. cache の read / write を abstract する port を定義する。
2. 保存対象は session-scoped の JSON cache だけに限定し、history へのミラーや shell history への書き込みは port に含めない。
3. port は atomic replace 前提で、読み取り側が壊れた file を直さない契約にする。

### 4.3 adapters

1. `ai` 側 adapter で JSON cache の path、0600、tmp file + rename、fsync 相当の手順を実装する。
2. `aish shell` の rcfile 注入は `shell_completion.rs` 側に閉じ、`ai complete` の出力を source するだけにする。
3. `ai complete` は既存の shell completion 出力を壊さず、末尾に recall hook trailer を append する。
4. bash / zsh の hook は idempotent にし、二重 source でも duplicate binding を作らない。

### 4.4 composition

1. `ai` の turn 完了時に recall cache save と stderr hint を呼ぶ。
2. `--quiet` は hint を止めるが、interactive TTY での cache save は止めない。
3. `--format json|tsv|env` は hint と cache save を両方止める。
4. non-TTY では recall を fail-closed にするが、`ai complete` の script 出力自体は継続可能にする。
5. `aish shell` と `ai complete` の双方が同じ hook 文字列を使うように合成する。

### 4.5 tests

1. domain の unit で extraction / normalization / queue 順 / oversize / control char の回帰を固定する。
2. adapter の unit で JSON schema、atomic write、0600、壊れた cache の扱いを固定する。
3. bash / zsh の hook 注入は integration で固定する。
4. `aish shell` と `ai complete` の hook 文面は同一 helper を参照していることを固定する。

### 4.6 Step 6 用の mock 導通コマンド

`smoke-mock.sh` とは別に、recall 専用の mock 導通コマンドを追加する。

- 例: `./scripts/recall-mock.sh`
- 目的: mock `aibe` へ 1 turn 送信し、cache に候補が保存され、`ai complete bash|zsh` が recall hook trailer を出すことを非対話で確認する
- 追加で確認する項目: `Alt+.` の binding 文面、bash / zsh の hook idempotency、unsupported shell での no-op

## 5. shell hook スニペット方針

### 5.1 共通方針

- `aish shell` は rcfile 注入を担当し、`ai` は hook 本文の正本を担当する。
- `ai complete` は既存 completion script を出した後、同じ recall hook trailer を末尾に append する。
- hook は `bash` / `zsh` の両方で idempotent にする。
- `Alt+.` は primary binding とし、実行ではなく prompt buffer への挿入だけを行う。
- hook が無い shell では no-op に落とし、`ai` の通常実行を壊さない。

### 5.2 bash 方針

- `bind -x` か同等の readline hook で `Alt+.` を結ぶ。
- 挿入先は `READLINE_LINE` と `READLINE_POINT` に限定する。
- function は cache から最新 queue を読むだけにし、history には触れない。

### 5.3 zsh 方針

- `bindkey` で `Alt+.` を結ぶ。
- 挿入先は `BUFFER` と `CURSOR` に限定する。
- `precmd_functions` / `preexec_functions` を増やす場合は既存配列への append のみとし、重複登録を防ぐ。

### 5.4 `shell_completion.rs` と `ai complete` の関係

- `aish/src/adapters/outbound/shell_completion.rs` は `aish shell` の rcfile に `ai complete bash|zsh` を評価させる。
- `ai/src/clap_cli.rs` の `run_complete` は completion script の生成に加えて recall hook trailer を出力する。
- どちらも同じ helper 文字列を使い、片方だけ更新して drift しないようにする。

## 6. cache 形式

JSON を正本にする。`AI_SUGGESTION_CACHE` が指す file は 1 session 1 file、0600、atomic replace とする。

### 6.1 schema

```json
{
  "schema_version": 1,
  "ai_session_id": "20260701abcd",
  "conversation_id": "optional-string-or-null",
  "shell": "bash",
  "updated_at": "2026-07-01T12:34:56Z",
  "queues": [
    {
      "turn_id": "550e8400-e29b-41d4-a716-446655440000",
      "captured_at": "2026-07-01T12:34:56Z",
      "candidates": [
        {
          "text": "git status",
          "language": "bash",
          "bytes": 10
        }
      ]
    }
  ]
}
```

### 6.2 取り決め

- `schema_version` は将来の migration 用に必須とする。
- `queues` は turn の出現順を保持し、最新 turn を優先する。
- `candidates` は assistant の出力順を維持する。
- shell hook はこの file を読むだけで、replay や history の永続化はしない。
- JSON 以外の形式は採らない。将来の拡張でも schema_version を上げて migration する。

## 7. 受け入れ条件

| ID | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| AC-01 | `ai` が final assistant content から bash / zsh fenced block を抽出し、cache に保存する | `extract_shell_candidates_from_fenced_code_blocks` | true |
| AC-02 | 複数 block が 1 turn にあっても queue 順を維持する | `preserve_suggested_command_queue_order_across_multiple_fences` | true |
| AC-03 | bash で `Alt+.` が `READLINE_LINE` に候補を挿入し、history を汚さない | `bash_alt_period_inserts_suggested_command_into_readline_line` | true |
| AC-04 | zsh で `Alt+.` が `BUFFER` に候補を挿入し、history を汚さない | `zsh_alt_period_inserts_suggested_command_into_buffer` | true |
| AC-05 | `aish shell` と `ai complete` の両方で同じ hook が入る | `aish_shell_and_ai_complete_install_the_same_recall_hook` | true |
| AC-06 | `--quiet` は hint を抑止するが、TTY では recall cache を維持する | `quiet_mode_suppresses_hint_without_disabling_recall_cache` | true |
| AC-07 | `--format json|tsv|env` は hint / cache を無効化する | `structured_output_disables_suggested_command_recall` | true |
| AC-08 | 非 TTY では recall が fail-closed になる | `non_tty_disables_suggested_command_recall` | true |
| AC-09 | unsupported shell では hook が入らないが、`ai` の通常実行は壊れない | `unsupported_shells_do_not_install_recall_hook` | true |
| AC-10 | 抽出候補の制御文字 / NUL / oversize が安全に拒否される | `reject_control_char_nul_and_oversized_suggested_commands` | true |

## 8. `scripts/spec-acceptance.toml` 登録案

`spec = "0053"` として追加し、初期値は **すべて `pending = true`** とする。  
未到達の AC は `#[ignore]` 付きテストを先に置き、Phase ごとに `pending = false` に切り替える。

| Phase | id | description | test | file_glob | pending |
|------|----|-------------|------|-----------|---------|
| 1 | `extract_shell_candidates` | fenced code block から shell candidate を抽出する | `extract_shell_candidates_from_fenced_code_blocks` | `ai/src/domain/suggested_command_recall.rs` | true |
| 1 | `suggested_command_queue_order` | 1 turn 内の複数 fence の queue 順を維持する | `preserve_suggested_command_queue_order_across_multiple_fences` | `ai/src/domain/suggested_command_recall.rs` | true |
| 1 | `reject_oversize_and_controls` | 制御文字 / NUL / oversize を拒否する | `reject_control_char_nul_and_oversized_suggested_commands` | `ai/src/domain/suggested_command_recall.rs` | true |
| 1 | `quiet_keeps_cache` | `--quiet` は hint を止めるが cache は維持する | `quiet_mode_suppresses_hint_without_disabling_recall_cache` | `ai/src/application/suggested_command_recall.rs` | true |
| 1 | `format_disables_recall` | `--format` は hint / cache を止める | `structured_output_disables_suggested_command_recall` | `ai/src/application/suggested_command_recall.rs` | true |
| 1 | `non_tty_fail_closed` | non-TTY では recall を fail-closed にする | `non_tty_disables_suggested_command_recall` | `ai/src/application/suggested_command_recall.rs` | true |
| 2 | `bash_readline_insert` | bash の `Alt+.` が `READLINE_LINE` を更新する | `bash_alt_period_inserts_suggested_command_into_readline_line` | `aish/src/adapters/outbound/shell_completion.rs` | true |
| 2 | `zsh_buffer_insert` | zsh の `Alt+.` が `BUFFER` を更新する | `zsh_alt_period_inserts_suggested_command_into_buffer` | `aish/src/adapters/outbound/shell_completion.rs` | true |
| 2 | `same_hook_from_aish_and_ai` | `aish shell` と `ai complete` が同じ hook を出す | `aish_shell_and_ai_complete_install_the_same_recall_hook` | `ai/src/clap_cli.rs` | true |
| 2 | `unsupported_shell_noop` | unsupported shell では hook を入れない | `unsupported_shells_do_not_install_recall_hook` | `ai/src/clap_cli.rs` | true |

## 9. `docs/architecture.md` 更新箇所

実装と同じ PR で次を更新する。

1. **依存ルール / 責務分離**
   - `ai` は recall 抽出と cache 保存を持つことを明示する。
   - `aish` は shell bootstrap と hook 配線だけを持ち、assistant content を解釈しないことを明記する。
2. **aish ログ / CLI 節**
   - `aish shell` の rcfile が `ai complete` の recall hook を注入することを追記する。
   - `Alt+.` が prompt buffer への挿入専用であることを追記する。
3. **`ai` の turn / completion 節**
   - `ai complete bash|zsh` が completion script の末尾に recall hook trailer を出すことを追記する。
   - `--quiet`、`--format`、non-TTY の recall gating を追記する。
4. **設定 / env 節**
   - `AI_SUGGESTION_CACHE`、`AI_SUGGESTED_COMMAND_RECALL`、`AI_SUGGESTED_COMMAND_RECALL_HINT` の意味を明記する。
5. **セキュリティ節**
   - recall cache を未信頼テキストとして扱い、自動実行しないことを明記する。

## 10. `docs/manual/ai-ux.md` 更新箇所

実機検証の節として、新しい「提案コマンド再呼び出し」章を追加する。

1. **前提**
   - `cargo build -p ai -p aibe -p aish` と TTY 前提を明記する。
   - `aish shell` 経由と `eval "$(ai complete bash|zsh)"` 経由の両方を試すことを明記する。
2. **確認項目**
   - assistant の final message に fenced bash / zsh block を出したとき、stderr hint が出ること。
   - `Alt+.` で prompt buffer に候補が挿入され、実行されないこと。
   - `--quiet` で hint が消え、`--format json|tsv|env` で recall が無効化されること。
   - non-TTY では recall が fail-closed になること。
3. **補足**
   - `aish shell` と `ai complete` が同じ hook 文面を使うことを手順の中で確認する。
   - history を汚さないことを明示する。

## 11. 仕様との差分

- なし。設計書の AC と shell hook 方針をそのまま実装指示へ落とす。
