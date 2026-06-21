# 0047 — `ai` 対話的プロンプト入力モード 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **元設計**: [0047_ai-interactive-prompt-input-spec.md](../spec/0047_ai-interactive-prompt-input-spec.md)  
> **状態**: 実装済み  
> **起票**: 2026-06-21  
> **関連**: [0027_ai-ux-spec.md](../spec/0027_ai-ux-spec.md)、[0030_ai-smart-entry-spec.md](../spec/0030_ai-smart-entry-spec.md)、[0047_ai-interactive-prompt-input-spec.md](../spec/0047_ai-interactive-prompt-input-spec.md)、[ai-ux.md](../manual/ai-ux.md)、[README.md](../../README.md)

## 0. 目的

`docs/spec/0047_ai-interactive-prompt-input-spec.md` を満たすために、`ai` に bare `ai` の対話的プロンプト入力を追加する。既存の `ai ask` / `stdin pipe` / `ai chat` を壊さず、`resolve_ask_message` / `normalize_args` / `run_ask` の責務を整理して、TTY 上の prompt 入力、外部エディタ、`reedline` ベースの内蔵ミニエディタを段階導入する。

## 1. Phase 分割

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | 入力ソース分類の導入。`normalize_args` の provenance 保持、`resolve_ask_message` の再設計、`run_ask` から bare `ai` と explicit `ai ask` を分岐できるようにする。TTY / pipe / argv / file の優先順位を固定し、prompt mode の入口だけを先に作る。 | `pending = true` の 1 系列がすべてテストとして存在し、`#[ignore]` 付きでもよい |
| 2 | 外部エディタ経路の実装。`AI_EDITOR` / `VISUAL` / `EDITOR` の解決、`shell-words` による 1 回分解、一時ファイル、コメントブロック除去、異常終了の fail-closed を実装する。 | Phase 2 の acceptance が `pending = false` になるまで Phase 3 に進まない |
| 3 | `reedline` による内蔵ミニエディタ、TTY 起動の E2E、manual 手順、README / `docs/manual/ai-ux.md` の更新を仕上げる。 | 全 acceptance が `pending = false` で `./scripts/verify.sh` が通る |

## 2. 受け入れ条件

設計書 §5 の 10 条件をそのまま 1:1 で登録する。未到達の AC は、先に Rust テストを `#[ignore]` 付きで追加してから実装に入る。

| ID | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| AC-01 | bare `ai` を TTY 実行したとき prompt UI が起動する | `bare_ai_tty_starts_prompt_mode` | true |
| AC-02 | bare `ai` の prompt 送信が `aibe` に 1 回だけ送られる | `bare_ai_prompt_message_is_sent_once` | true |
| AC-03 | `echo ... | ai` は pipe 入力として処理され、prompt UI を出さない | `pipe_input_keeps_existing_ask_path` | true |
| AC-04 | `ai hello` と `ai 'hello'` は argv 入力として処理される | `argv_input_keeps_existing_ask_path` | true |
| AC-05 | `ai ask` / `ai --help` / `ai --version` / subcommands / option-only では prompt UI を出さない | `explicit_invocations_do_not_enter_prompt_mode` | true |
| AC-06 | `AI_EDITOR` / `VISUAL` / `EDITOR` の優先順位で外部エディタが選ばれる | `editor_precedence_prefers_ai_editor_then_visual_then_editor` | true |
| AC-07 | 外部エディタで comment 行除去後が空なら AI を呼ばない | `empty_prompt_after_comment_strip_is_rejected` | true |
| AC-08 | 外部エディタが異常終了したら AI を呼ばない | `abnormal_editor_exit_is_rejected` | true |
| AC-09 | 内蔵ミニエディタで `Enter` / `Ctrl+D`・`Alt+Enter` / `Ctrl+C` の挙動が仕様どおりになる | `reedline_prompt_editor_handles_enter_eof_and_interrupt` | true |
| AC-10 | `ai chat` の既存 REPL と `stdin pipe` の既存挙動が回帰しない | `chat_repl_and_pipe_input_regression_guard` | true |

### `scripts/spec-acceptance.toml` 登録案

`spec = "0047"` で 10 件を追加する。Phase ごとの `pending` は次の通り。

- Phase 1: `AC-01` `AC-02` `AC-03` `AC-04` `AC-05` を `pending = true` で追加し、まずテストを赤で置く
- Phase 2: `AC-06` `AC-07` `AC-08` を `pending = true` で追加し、実装後に `false` へ切り替える
- Phase 3: `AC-09` `AC-10` を `pending = true` で追加し、TTY / manual まで通したら `false` にする

## 3. 実装順序

1. `ai/src/clap_cli.rs` と `ai/src/main.rs` の入口を分け、bare root invocation の provenance を失わないようにする
2. `ai/src/application/` に prompt 入力 orchestration を置き、`resolve_ask_message` の責務を入力ソース分類へ分割する
3. `ai/src/ports/` に prompt input / editor / temp file / tty 判定の trait を追加する
4. `ai/src/adapters/` に外部エディタ、コメント除去、`reedline` prompt editor を追加する
5. `run_ask` へ prompt mode を差し込み、dry-run / aibe 接続前に空入力・異常終了を fail-closed で弾く
6. `ai/tests/` に acceptance テストと回帰テストを追加する
7. `README.md`、`docs/manual/ai-ux.md`、必要なら CLI help 文言を更新する
8. `scripts/spec-acceptance.toml` を `pending = false` へ切り替えて `./scripts/verify.sh` を通す

## 4. ファイル配置

### 4.1 `application`

- `ai/src/application/ask_prompt_input.rs`
  - bare root invocation / explicit invocation / pipe / argv / file の分類
  - prompt mode の起動判定
  - `run_ask` 用の `ResolvedAskInput` 構築
- `ai/src/application/ask.rs`
  - 既存の `Ask` 実行フローに prompt input を差し込む
  - `resolve_ask_message` を薄い facade に縮める、または `ask_prompt_input` へ委譲する
- `ai/src/application/mod.rs`
  - 新モジュール公開

### 4.2 `ports`

- `ai/src/ports/inbound/prompt_input.rs`
  - prompt input の高レベル port
  - `PromptInputSource` / `PromptInputRequest` / `PromptInputResult`
- `ai/src/ports/outbound/editor.rs`
  - 外部 editor 起動の port
  - `AI_EDITOR` / `VISUAL` / `EDITOR` の選択結果
- `ai/src/ports/outbound/temp_file.rs`
  - 一時ファイル生成と `.md` 拡張子の保証
- `ai/src/ports/outbound/tty.rs`
  - `stdin` が TTY かどうかの抽象化
- `ai/src/ports/outbound/comment_filter.rs`
  - `<!-- ai-prompt: ... -->` ブロック除去

### 4.3 `adapters`

- `ai/src/adapters/inbound/reedline_prompt.rs`
  - `reedline` ベースの内蔵ミニエディタ
  - `Enter` / `Ctrl+D` / `Alt+Enter` / `Ctrl+C` / multi-line 入力
- `ai/src/adapters/outbound/external_editor.rs`
  - shell-words で editor command を 1 回だけ分解して起動
  - 起動失敗、非 0 終了、シグナル終了、読み戻し失敗を fail-closed にする
- `ai/src/adapters/outbound/comment_filter.rs`
  - comment block を送信前に除去する実装
- `ai/src/adapters/outbound/terminal_prompt.rs`
  - prompt mode の TTY 実装を束ねる façade。`chat_line_editor.rs` とは共有しない

### 4.4 既存関数への組み込み

- `normalize_args`
  - bare `ai` を一律 `ai ask` にしない
  - 代わりに「bare root」「explicit ask」「option-only」を区別できる形にする
- `resolve_ask_message`
  - `message missing` を直接返すのではなく、`argv` / `pipe` / `file` / `prompt` の分類結果を返す
  - explicit `ai ask` の message 不足は既存の usage error を維持する
- `run_ask`
  - prompt mode 判定を `resolve_ask_message` の後段ではなく、入力分類の結果に基づいて行う
  - prompt 完了前に `aibe` 接続・履歴記録へ進まない

## 5. テスト一覧

### Phase 1

- `ai/src/clap_cli.rs`
  - `normalize_args` の provenance を壊さないテスト
  - option-only / help / version の分岐テスト
- `ai/src/application/ask_prompt_input.rs`
  - bare root / explicit ask / pipe / argv の分類テスト
  - `resolve_ask_message` 相当の優先順位テスト

### Phase 2

- `ai/src/adapters/outbound/external_editor.rs`
  - `AI_EDITOR` > `VISUAL` > `EDITOR`
  - shell-words の 1 回分解
  - comment block 除去
  - empty / abnormal exit の fail-closed

### Phase 3

- `ai/src/adapters/inbound/reedline_prompt.rs`
  - `Enter` / `Ctrl+D` / `Alt+Enter` / `Ctrl+C`
  - multi-line 入力
- `ai/tests/`
  - TTY 付き bare `ai` の smoke
  - `echo ... | ai` の回帰
  - `ai chat` の回帰

## 6. ドキュメント更新

同じ変更で次を更新する。

- `README.md`
  - `ai` の日常導線に bare `ai` の対話入力を追加する
  - `AI_EDITOR` / `VISUAL` / `EDITOR` の優先順位を明記する
- `docs/manual/ai-ux.md`
  - bare `ai` の手動検証手順を追加する
  - `ai chat` 既存手順は維持しつつ、prompt mode の確認を追加する

## 7. 仕様との差分（意図的に縮小する場合のみ）

- なし

## 8. 注意

- `docs/0000_spec-index.md` はこの段階では更新しない
- `ai chat` の `rustyline` 経路は共有しない
- `shell-words` は workspace 依存として追加し、`ai` では直接パース実装を持たない
