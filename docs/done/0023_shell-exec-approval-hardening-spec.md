# 0023 — `shell_exec` 承認 UI hardening 指示書

> **出典**: `docs/security.md`、`docs/testing.md`、`docs/architecture.md`、`docs/manual/ai-ask-tools.md`、既存実装（`ai/src/adapters/outbound/aibe_client.rs`、`aibe-client/src/transport.rs`、`aibe/tests/shell_exec_approval_socket.rs`、`aish/src/domain/sanitize.rs`）。
>
> **状態**: 実装済み
>
> **重要**: 本書は **`0020` の差分** である。`shell_exec` 承認ポリシー本体・監査・同一 Unix 接続での prompt/approval 往復・`aibe` server 側 enforcement の正本は [0020_p4-daily-use-polish-spec.md](done/0020_p4-daily-use-polish-spec.md) に残す。本書が新たに閉じるのは **0020 実装後に残ったギャップ**（非対話 stdin、表示偽装、`aibe-client` 側テスト薄さ）のみ。`0021` Tab 補完、`cargo run` 補完スクリプト、`shell_quote` は **非対象**。

## 目的

`0020` で導入済みの `ai ask` 承認 UI に対し、**docs と実装の不整合**（`docs/security.md` は fail-closed 済み記載だが `ai` は pipe へ `y` を受け付けうる）を解消する。

本件で **新規に** 閉じるのは次の 3 点のみ。

1. 非対話 stdin（`stdin.is_terminal() == false`）での **fail-closed**（`read_line` 前に deny）
2. 承認プロンプトの `command` / `args` の **表示 hardening**（制御文字が端末制御として解釈されない形式）
3. **`aibe-client::agent_turn`** の承認往復を `UnixStream::pair` 統合テストで固定（`0020` の server テスト `shell_exec_approval_socket.rs` を補完）

## スコープ

### 対象

| ID | 対象 | 要点 |
|----|------|------|
| P4.5-1 | `ai` 承認 UI | `stdin.is_terminal()` で非対話 stdin を検出し、承認を拒否する |
| P4.5-2 | `ai` 表示安全化 | `command` / `args` を raw `{}` で出さず、制御文字が見える形式にする |
| P4.5-3 | `aibe-client` transport テスト | `0020` で実装済みの往復を **client クレート** で契約固定（wire 再設計はしない） |
| P4.5-4 | docs 同期 | `security.md` / `testing.md` / `architecture.md` / `docs/manual/ai-ask-tools.md` を最小限更新する |

### 非対象

| ID | 非対象 | 理由 |
|----|--------|------|
| P4.6 | Tab 補完 | 別タスクで完了済みの系統。承認 UI hardening とは独立 |
| `cargo run` 補完スクリプト | 実行補助の別経路 | 本件は承認 UI の安全化のみ |
| `shell_quote` 再設計 | 引数の shell quoting 変更 | 表示安全化とは別問題であり、本書では扱わない |
| `aibe` の承認プロトコル再設計 | `ClientRequest::ShellExecApproval` / `ClientResponse::ShellExecApprovalPrompt` の wire 再定義 | 既存 wire で足りる前提をまず使う |

## 受け入れ条件

### fail-closed（`ai`）

- `printf 'y\n' | ai ask ...` のように stdin が pipe の場合、`y` を入力しても承認にはならない。
- 非対話 stdin では `stdin.is_terminal() == false` を根拠に **読む前に** deny する（`read_line()` に依存しない）。
- deny 時 stderr: `ai: shell_exec denied (non-interactive stdin)`（既存 `n == 0` 時と同系統。実装は `is_terminal` 判定に統合してよい）。

### 表示 hardening（`ai`）

- `command` / `args` は raw `{}` 表示を使わない。
- 可視化は **`std::ascii::escape_default` を各フィールドに適用し、UTF-8 として lossy 表示する** 方式に固定する（`Debug` のみは不可）。
- 観測可能な合格例: 入力 `command` に `\x1b[31m` を含むとき、stderr 出力に **生の ESC がそのまま出ず**、`\x1b` または `\\x1b` 相当の escape 表現が含まれること。改行 `\n` を含む `arg` は **複数行に分割されず** 1 行の escape 文字列として出ること。
- unit テストで上記 2 観点を固定する。

### `aibe-client` 統合テスト（必須 2 本）

1. **承認あり**: `agent_turn` → `ShellExecApprovalPrompt` 受信 → callback `true` → `ShellExecApproval` 送信 → **同一接続**で final `agent_turn_result` 受信。途中で再接続しない。
2. **承認なし**: 同上で callback `false` → 最終 `agent_turn_result` まで **同一接続**で返る（拒否でも transport は閉じない）。

各テストで `id` / `turn_id` / `tool_call_id` / `command` / `args` が prompt から approval まで壊れないことを assert する。

### docs

- `docs/manual/ai-ask-tools.md` に、非対話 stdin の fail-closed と stderr-only 表示の確認手順を追加する。
- `docs/testing.md` に「`ai` = TTY/表示、`aibe-client` = socket 往復」と 1 行ずつ役割を追記する。

## 実装タスク分解

### ai

- `ai/src/adapters/outbound/aibe_client.rs` の `prompt_shell_exec_approval()` を hardening する。
- `stdin.is_terminal()` を最初に判定し、非対話なら **読む前に** deny する。
- 表示は `command` / `args` を安全な整形関数へ通してから `stderr` に出す。
- 現在の `stdin.read_line()` 失敗時の deny も維持するが、非対話判定とは切り分ける。
- `ai` の外部 I/O なので、UI の最終判断はここに置き、`aibe-client` に terminal 判定を押し込まない。

### aibe-client

- `aibe-client/src/transport.rs` の `agent_turn()` は UI 非依存の transport として維持する。
- `ShellExecApprovalPrompt` の wire 変換と `ShellExecApproval` の返送が、同一 socket 上で往復することをテストで固定する。
- transport そのものに TTY 判定や表示ロジックを追加しない。

### docs

- `docs/security.md` に、`shell_exec_approval = "ask"` の non-interactive stdin は fail-closed であることを明記する。
- `docs/architecture.md` に、承認 UI は `ai` 側の責務であり、`aibe-client` は transport のみを担うことを最小限追記する。
- `docs/testing.md` に、`aibe-client` の承認往復統合テストと `ai` 側の fail-closed unit/adapter テストを追記する。
- `docs/manual/ai-ask-tools.md` に、非対話 stdin と stderr-only プロンプトの手動確認を追加する。

### tests

- `ai` の unit / adapter テストで、非対話 stdin を fail-closed とする分岐を固定する。
- `aibe-client/tests/` に、`agent_turn` の承認往復統合テストを追加する。
- `docs/manual/ai-ask-tools.md` の手順は、手動検証の正本として実装テストと矛盾させない。

## `stdin.is_terminal()` による fail-closed の具体挙動

- 判定は `read_line()` より前に行う。
- `stdin.is_terminal() == false` の場合は、入力を読まずに deny する。
- このときの stderr 文言は、既存の `ai ask` の deny 文脈に合わせて `ai: shell_exec denied (non-interactive stdin)` 系の 1 行とする。
- 端末での `read_line()` 失敗は別系統の異常として扱い、既存の `ai: shell_exec denied (stdin unavailable)` 系の deny 文言を維持する。
- deny は `agent_turn` の拒否結果として処理し、`ai ask` のプロセス終了コードを新しく分岐させない。成功した turn は従来どおり `ExitCode::SUCCESS`、transport / request / response の実エラーのみ `ExitCode::FAILURE` に落ちる。

## 承認表示の安全化方針

### 採用方式（固定）

- `std::ascii::escape_default` を各 `command` / 各 `arg` に適用し、`String::from_utf8_lossy` で 1 行ずつ `stderr` に出す（例: `command: \x1b[31mls\x1b[0m` のような見え方）。
- `command` と `args` は行を分ける（`args:` の後は要素ごとに escape 済み文字列を列挙、または `Debug` 風の `[...]` 1 行でもよいが **中身は必ず escape 済み**）。

### 再利用可否の調査結果

- `aish/src/domain/sanitize.rs` の `sanitize_log_text()` は、秘密情報のマスクには再利用できる。
- ただし `sanitize_log_text()` は制御文字可視化の機能を持たないため、これ **だけ** では ANSI 偽装対策にならない。
- したがって、`aish` の関数をそのまま流用するのではなく、`ai` 側で `sanitize_log_text()` 相当のマスクと制御文字可視化を **組み合わせる** か、`ai` 内に小さな共通 formatter を切る。
- `ai` から `aish` へ新しい依存を増やすのは避ける。

## `aibe-client` 統合テスト方針

`aibe-client` は TTY を知らない transport 層なので、テストはプロセス起動ではなく `UnixStream::pair()` で閉じる。

### ねらい

- 承認 prompt が来たら、callback が呼ばれることを確認する
- callback の戻り値が `ShellExecApproval` として返送されることを確認する
- 返送後に final `agent_turn_result` が 1 接続上で届くことを確認する
- `id` / `turn_id` / `tool_call_id` / `command` / `args` が途中で壊れないことを確認する

### 必須テスト（受け入れ条件と同一）

| # | callback | 期待 |
|---|----------|------|
| 1 | `true` | prompt → `ShellExecApproval { approved: true }` → final `agent_turn_result`（同一 socket） |
| 2 | `false` | prompt → `ShellExecApproval { approved: false }` → final `agent_turn_result`（同一 socket） |

server fixture は `aibe/tests/shell_exec_approval_socket.rs` の NDJSON 行形式に合わせる（**aibe バイナリは起動しない**）。

## テスト計画

### unit

- `ai/src/adapters/outbound/aibe_client.rs`
  - 非対話 stdin を deny する分岐
  - `command` / `args` の安全表示
- `aish/src/domain/sanitize.rs`
  - 既存の秘密情報マスク契約の回帰は維持する

### integration

- `aibe-client/tests/agent_turn_approval.rs`
  - `UnixStream::pair()` で承認 prompt -> approval -> final response を往復する
- `ai/tests/`（新規 `shell_exec_approval_ui.rs` 等でも可）
  - 非対話 stdin の fail-closed（subprocess + pipe で `printf y` を流す）
  - 制御文字入り prompt の stderr 出力が escape されること
- `ask_integration.rs` は **本書では必須変更にしない**（既存契約の回帰のみ）

### manual

- `docs/manual/ai-ask-tools.md`
  - `shell_exec_approval = "ask"` で `Execute? [y/N]` が stderr に出ること
  - `printf 'y\n' | ai ask ...` では承認されないこと
  - 制御文字を含む `command` / `args` が raw に見えないこと

## docs 更新対象

| ファイル | 変更内容 |
|----------|----------|
| `docs/security.md` | `shell_exec_approval = "ask"` の fail-closed、承認プロンプトの安全表示方針 |
| `docs/testing.md` | `ai` / `aibe-client` の新しい unit / integration の置き場所 |
| `docs/architecture.md` | `ai` が承認 UI を持ち、`aibe-client` が transport に徹すること |
| `docs/manual/ai-ask-tools.md` | 非対話 stdin の fail-closed と stderr-only 表示の確認手順 |

## 未確定・推測・指示外

- `stdin.is_terminal()` の具体的な実装位置は、`prompt_shell_exec_approval()` の内部 helper か、テスト容易化のための小さな分離関数か、実装時に判断が必要である。**推測**としては helper 分離が最もテストしやすい。
- `aish/src/domain/sanitize.rs` を `ai` から再利用するかは、クレート境界と依存方向の都合を見て最終判断する必要がある。現時点では直接依存を増やさない方針を優先する。
- `command` / `args` の可視化は最低 `Debug` を満たせばよいが、どの escape 形式に揃えるかは実装時の UX 調整が残る。
- `aibe-client/tests` のファイル名と、server 側の fixture の切り方は、既存のテスト配置に合わせて最終調整する必要がある。

## 残リスク

- 手動検証では、実際に tty / pipe を切り替えて fail-closed を確認する必要がある。
- `escape_default` 表示は可読性より安全優先。UX 調整は本書スコープ外。
- `aibe-client` の統合テストは socket 往復を固定するが、実端末の入力取得までは代替しない。
