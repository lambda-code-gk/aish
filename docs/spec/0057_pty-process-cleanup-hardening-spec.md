# 0057 PTY Process Cleanup Hardening 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-07-10  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)

## 0. Core outcome

0055 の同期型 human handoff が timeout / SIGINT / SIGTERM / 異常終了しても、有限時間内に `ai`・`aish human-shell`・PTY shell・通常の foreground/background jobs を片付け、端末 termios を復元し、runtime handoff dir を削除する。

## 1. Minimum vertical slice

```text
ai --collaborative --timeout
→ shell_exec approval が synchronous human handoff を開始
→ timeout が ai の cancel flag を立てる
→ AishHumanShellLauncher が子 aish へ cancel を伝播して有限時間で待つ
→ aish human-shell が PTY shell / foreground job / 通常 background job を bounded termination で終了・回収する
→ ai が handoff runtime dir を cleanup して timeout 終了を返す
```

最小 slice は `handoff_timeout_terminates_bounded` を縦断 AC とする。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に、human handoff 中の終了制御だけを追加で保証する。対象は単一ホスト・単一ユーザー・単一 handoff プロセスツリーで、`ai` と `aish human-shell` のプロセスが cleanup を実行できる状態で生存しているケースに限る。

- `ai --timeout`、`ai` への SIGINT / SIGTERM、handoff 起動・待機中の異常終了を cancel として扱う
- `AishHumanShellLauncher::launch_and_wait()` は `Command::status()` の無期限待ちを使わず、spawn + poll + cancel flag で有限時間終了する
- `aish human-shell` は PTY shell と同一 session / 通常 process group に残る foreground/background jobs を、SIGHUP / SIGTERM / PTY master close / WNOHANG reap / SIGKILL escalation / 期限付き reap で片付ける
- `aish` は直接の子を zombie にしない
- `aish` 側の raw mode guard に加え、親側 `ai` でも handoff 前後の termios を復元できる
- `handoff_service.execute()` が返らない経路でも、`ai` は runtime handoff dir cleanup に到達する
- 正常 handoff（Ctrl+D / `exit`）の 0055 契約は壊さない

### 2.2 保証対象外

- `setsid` で別 session へ脱出したプロセス
- double-fork daemon / nohup daemon の完全掃除
- 権限移譲後のプロセス
- 別 PID namespace / container / VM 内のプロセス
- cgroup による完全な descendant containment
- `ai` / `aish` 自身が SIGKILL された場合の cleanup 実行
- OS クラッシュ後・プロセスクラッシュ後の durable recovery

## 3. Non-goals

- resume / durable workflow / `ai resume`
- side agent / secondary agent loop
- child Work 統合
- lease / heartbeat / reconciler
- handoff 履歴永続化
- 複数 human shell 並列実行
- 監視デーモン
- cgroup 必須化
- 正常系 handoff UX の新機能追加

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 2（`ai`、`aish human-shell`） |
| 状態機械 | 0（永続状態なし。bounded cleanup は deadline 付き手続き） |
| 永続 aggregate | 0 |
| 外部副作用 | 3（process signal / reap、termios restore、runtime dir cleanup） |
| プロセス境界 | 2（`ai`→`aish human-shell`、`aish`→PTY shell/jobs） |
| 新規基盤機構 | bounded-pty-handoff-cleanup |
| 他機能統合 | 2（`ai` human handoff、`aish` PTY shell） |

`scripts/feature-scope.toml` の `0057` entry と一致させる。

## 5. Complexity Gate

- 判定: Yellow
- 理由: 新しい永続状態や実行主体は追加しないが、`ai` と `aish` をまたぐ process cleanup で process boundary / integration / external effect が Yellow 閾値に達する
- 分割判断: 0055 から defer された hardening だけに限定し、side agent / durable recovery / cgroup は別 spec へ送るため単一 spec として進める
- 承認例外: なし（Yellow として `scope_review = "approved"`）

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 状態機械 | +0 |
| 永続 aggregate | +0 |
| 新規実行主体 | +0 |
| 新規 agent loop | +0 |
| lease / heartbeat / reconciler | +0 |
| cgroup 必須化 | +0 |
| protocol DTO 追加 | +0（既存 handoff failure / interrupted 経路で表現） |

## 7. Split triggers

次が必要になったら STOP-THE-LINE し、別 spec へ分割する。

- cgroup を必須 containment として導入する
- cleanup 専用 daemon / watchdog process を追加する
- handoff 状態を永続化して crash recovery する
- lease / heartbeat / reconciler が必要になる
- 複数 human handoff の並列管理が必要になる
- `setsid` 脱出や double-fork daemon の完全掃除を保証対象に入れる
- aibe protocol に新しい handoff lifecycle DTO が必要になる

## 8. パック構成の適用

**No** — 0057 は optional 機能の脱着ではなく、0055 human handoff の core safety hardening である。`aish` の PTY cleanup は全 build で必要なプロセス安全境界であり、0045 §6 の「`aish` に載せる機能は対象外」にも該当するため、Pack 境界 / Active Pack / Basic Pack は作らない。

## 9. Acceptance Criteria

| ID | 条件 |
|----|------|
| `handoff_timeout_terminates_bounded` | `ai --collaborative --timeout` 中に human handoff が active でも、外側 watchdog より短い有限時間で non-zero 終了する |
| `external_sigint_sigterm_stops_handoff` | `ai` へ外部 SIGINT / SIGTERM を送ると、handoff が有限時間で停止し、通常成功として扱われない |
| `aish_human_shell_and_pty_shell_reaped` | cancel / timeout 後に `aish human-shell` と PTY shell プロセスが残らない |
| `foreground_and_normal_background_jobs_terminated` | PTY shell 内で起動した foreground job と通常 background job が cancel / timeout 後に残らない |
| `sigterm_ignored_escalates_sigkill` | SIGTERM / SIGHUP を無視する job は期限後に SIGKILL escalation され、終了が完了する |
| `direct_children_reaped_no_zombies` | `aish` が直接 spawn / fork した子を zombie として残さない |
| `terminal_echo_canonical_restored` | handoff が timeout / SIGINT / SIGTERM / 異常終了しても、親端末の echo と canonical mode が復元される |
| `runtime_handoff_dir_removed_on_abort` | `handoff_service.execute()` が正常 return しない経路でも runtime handoff dir が削除される |
| `normal_handoff_success_path_unchanged` | Ctrl+D / `exit` の正常 handoff は 0055 と同じ `human_control_returned` と再観測結果を返す |
| `cleanup_e2e_has_outer_watchdog` | 実 PTY E2E は外側 watchdog を持ち、ハング時にテスト全体を無制限に止めない |

各 row は `scripts/spec-acceptance.toml` に `pending = true` として登録し、実装開始時に対応する `#[ignore]` テストを外していく。

## 10. Deferred specs

- cgroup による descendant containment 強化
- `setsid` / double-fork daemon の検出・報告
- durable handoff recovery / resume
- 複数 human shell の並列管理
- handoff lifecycle の詳細 telemetry / 履歴永続化

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | 0055 follow-up hardening として timeout / signal / process cleanup / termios / runtime dir cleanup の AC を固定 | 0055 で明示 defer された cleanup 保証を独立 spec として設計確定するため |
| 2 | SAFETY_WITHIN_FAULT_MODEL | external effect 数を 2 から 3 へ補正 | process signal / reap、termios restore、runtime dir cleanup は別系統の副作用として registry と設計書を一致させるため |
