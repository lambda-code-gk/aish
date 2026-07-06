# 0055 Minimal Human Handoff 設計書

## 目的

親 collaborative エージェントが要求した `shell_exec` を自動実行せず、人間の対話シェルへ制御を渡し、Ctrl+D または `exit` で同じ親エージェントへ制御を戻す。

親は human shell から制御が返った後、要求コマンドの成功を仮定せず、現在環境を再観測して処理を続ける。

## スコープ

### 含む

- 同期型 human handoff（親プロセスが直接 spawn + wait）
- `ai --collaborative` による親 agent の `shell_exec` インターセプト
- 実 PTY human shell（aish `human-shell`）
- 候補コマンドの表示（自動実行なし）
- 再観測（cwd / Git / shell log 末尾）
- synthetic tool result（`human_control_returned` / `requested_command_completion = unknown`）

### 含まない（別 spec へ分離）

- side agent / `request_human_action`
- durable workflow / crash recovery / `ai resume`
- child Work 統合
- lease / heartbeat / token rotation
- 永続 handoff ファイル（`handoff.json` 等）
- 永続 candidate queue

## パック構成の適用

**No** — collaborative handoff は `ai` / `aish` / `aibe-protocol` の協調経路であり、optional Pack 脱着の対象ではない。core 固定で同期フローのみ提供する。

## 状態モデル

永続状態機械は作らない。処理中は call stack とローカル変数のみ。

```text
ParentRunning → HumanActive → ParentRunning
```

異常終了は永続化せずエラーとして返す。

## CLI

```bash
ai --collaborative "..."
```

`role == Parent` かつ `collaborative == true` の agent turn のみ `shell_exec` を human shell へ変換する。

## プロトコル

`ShellExecApproval` に `handoff_result: Option<HumanHandoffResult>` を追加。

`HumanHandoffResult` 必須フィールド:

- `execution_outcome = human_control_returned`
- `requested_command_completion = unknown`
- `human_shell_exit_code`（成功判定に使わない）
- `final_shell_cwd`
- `shell_log_range`
- `observation`（再観測サマリ）

## セキュリティ

human shell へ渡す環境変数は `AISH_CONTROL_MODE=human-shell` と handoff 表示用 hint のみ。親の秘密情報・token・memory 内容は渡さない。

## 受け入れ条件

1. `--collaborative` 親の `shell_exec` のみ human shell へ変換される
2. 通常 `shell_exec` / 通常 `ai` に回帰がない
3. human shell は要求 cwd で実 PTY 起動する
4. 候補コマンドは自動実行されない
5. Ctrl+D / `exit` で親へ戻る
6. shell exit code を要求コマンド成功とみなさない
7. 親が再観測結果を受け取る
8. 永続 handoff state を作成しない
9. 実 PTY E2E が成功する
10. `./scripts/verify.sh` が成功する

## 将来拡張

- **0055B** Side Agent Collaboration
- **0055C** Durable Handoff Recovery
