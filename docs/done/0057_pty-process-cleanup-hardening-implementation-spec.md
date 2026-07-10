# 0057 PTY Process Cleanup Hardening 実装指示書

設計書: [`docs/spec/0057_pty-process-cleanup-hardening-spec.md`](../spec/0057_pty-process-cleanup-hardening-spec.md)

## 0. 目的

0055 の同期型 human handoff が timeout / SIGINT / SIGTERM / 異常終了した場合でも、`ai`、`aish human-shell`、PTY shell、通常の foreground/background jobs を有限時間で片付け、端末 termios と runtime handoff dir cleanup を保証する。設計書を正本とし、0057 の Fault Model と Non-goals を緩めない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: `2`
- Complexity class: Yellow（`scope_review = "approved"`）
- Vertical slice AC ID: `handoff_timeout_terminates_bounded`
- Locked AC IDs:
  - `handoff_timeout_terminates_bounded`
  - `external_sigint_sigterm_stops_handoff`
  - `aish_human_shell_and_pty_shell_reaped`
  - `foreground_and_normal_background_jobs_terminated`
  - `sigterm_ignored_escalates_sigkill`
  - `direct_children_reaped_no_zombies`
  - `terminal_echo_canonical_restored`
  - `runtime_handoff_dir_removed_on_abort`
  - `normal_handoff_success_path_unchanged`
  - `cleanup_e2e_has_outer_watchdog`

## 1. Phase 分割

| Phase | 内容 | ゲート |
|-------|------|--------|
| 1 | timeout vertical slice: `ai --collaborative --timeout` の cancel flag を `HumanShellLauncher` へ接続し、`AishHumanShellLauncher` を spawn + poll 化して子 `aish human-shell` を有限時間で停止する。runtime dir cleanup guard と外側 watchdog 付き E2E を先に通す | `handoff_timeout_terminates_bounded`, `runtime_handoff_dir_removed_on_abort`, `cleanup_e2e_has_outer_watchdog` |
| 2 | aish PTY cleanup: `terminate_pty_session` と bounded `kill_and_wait` を実装し、PTY shell / foreground job / 通常 background job / SIGTERM 無視 job / direct child reap を保証する | `aish_human_shell_and_pty_shell_reaped`, `foreground_and_normal_background_jobs_terminated`, `sigterm_ignored_escalates_sigkill`, `direct_children_reaped_no_zombies` |
| 3 | 外部 signal と terminal hardening: `ai` への SIGINT / SIGTERM を handoff cancel へ伝播し、aish RAII + ai 親側 termios 復元の二重防御を入れ、正常 handoff regression を確認する | `external_sigint_sigterm_stops_handoff`, `terminal_echo_canonical_restored`, `normal_handoff_success_path_unchanged` |

### Vertical Slice Gate

Phase 1 は `handoff_timeout_terminates_bounded` を最小 vertical slice とする。Phase 1 成功前に、cgroup containment、durable recovery、resume、並列 handoff、監視 daemon、handoff lifecycle DTO 追加を実装してはならない。

### Phase 完了順序

各 Phase では次の順序を守る。

1. 対応する skeleton test を本物のテストへ置き換える。
2. 実装が通ったら `scripts/spec-acceptance.toml` の該当 AC だけ `pending = false` にする。
3. 同じ変更で Rust 側の `#[ignore]` を外す。
4. `./scripts/verify-targeted.sh` または対象クレートの `cargo test -j 1 ...` で確認する。

全 Phase 完了後にのみ、全 0057 AC を `pending = false` にし、`./scripts/verify.sh` を成功させる。

## 2. 変更対象ファイル

### ai

- `ai/src/ports/outbound/human_handoff.rs`
  - `HumanShellLauncher::launch_and_wait` に cancel 引数を追加する。
  - `HumanShellLaunchError::Cancelled(String)` を追加する。
- `ai/src/application/human_handoff.rs`
  - `RunSynchronousHumanHandoff::execute` が cancel flag を受け取り、launcher へ渡す。
  - `Cancelled` は user denial ではなく handoff failure / interrupted 経路として扱う。
- `ai/src/adapters/outbound/human_handoff.rs`
  - `AishHumanShellLauncher::launch_and_wait` を `Command::status()` から `spawn` + `try_wait` poll に変更する。
  - cancel 検知時は子 `aish human-shell` へ SIGTERM、grace、SIGKILL、期限付き wait の順で停止する。
  - `cleanup_runtime_handoff_dir` は abort 経路でも実行される guard（Drop / scopeguard 相当）を使う。新規依存を足すより小さい local RAII guard を優先する。
- `ai/src/main.rs`
  - `_cancel_requested_thread` を実際に `handoff_service.execute(...)` へ接続する。
  - timeout、SIGINT、SIGTERM、handoff failure の各経路で同じ cancel flag が handoff launcher へ見えるようにする。
  - handoff 前後に親側 termios を保存・復元する guard を入れる。非 TTY では no-op。
- `ai/tests/0057_pty_process_cleanup_hardening.rs`
  - `scripts/spec-acceptance.toml` に登録済みの 6 テストを実装する。
  - 実 PTY E2E は外側 watchdog を必須にし、ハング時は process group を kill してから panic する。
- 参照: `ai/tests/0055_collaborative_handoff_vertical_e2e.rs`
  - `e2e_timeout`, `wait_child_with_timeout`, `kill_process_group_and_reap`, mock aibe server の timeout パターンを流用する。

### aish

- `aish/src/adapters/outbound/pty_shell.rs`
  - `terminate_pty_session` を追加し、PTY master close / SIGHUP / SIGTERM / WNOHANG reap / SIGKILL escalation / 期限付き wait を一箇所に集約する。
  - 既存 `kill_and_wait` は無期限 wait から bounded `kill_and_wait(child, grace, deadline)` へ置き換える。
  - `relay_master_fd` の終了・エラー・cancel 相当経路で direct child を zombie にしない。
  - `StdinTermiosGuard` の RAII 復元は維持し、panic / error 経路でも drop される構造を崩さない。
- `aish/src/human_shell.rs`
  - `run_human_shell` の異常終了時に PTY cleanup が走るよう、`RunShell` / `PtyShell` 側の cancel-safe cleanup と整合させる。
  - result file は正常 return marker がある場合だけ書く 0055 契約を壊さない。
- `aish/tests/0057_pty_process_cleanup_hardening.rs`
  - `scripts/spec-acceptance.toml` に登録済みの 4 テストを実装する。
  - helper process を使い、テスト本体が cleanup 対象の process group に巻き込まれない構成にする。
- 参照: `aish/tests/0055_minimal_human_handoff.rs`, `aish/tests/shell_interactive.rs`
  - 既存 PTY / timeout helper の作法を踏襲する。

## 3. API 変更方針

`HumanShellLauncher` は以下の方向で変更する。

```rust
fn launch_and_wait(
    &self,
    request: &HumanShellLaunchRequest,
    cancel_requested: &std::sync::atomic::AtomicBool,
) -> Result<HumanShellReturn, HumanShellLaunchError>;
```

実装都合で `Arc<AtomicBool>` や小さな `CancellationToken` trait にする場合でも、次を満たすこと。

- `ai/src/main.rs` の turn-level cancel flag と同じ状態を参照する。
- timeout / SIGINT / SIGTERM で launcher の poll loop が有限時間内に気づく。
- `Cancelled` は `MissingReturnMarker` と区別できる。
- `Command::status()` の無期限待ちは使わない。

`AishHumanShellLauncher` の poll loop は 50-100ms 程度の sleep / poll 間隔とし、cancel 検知後は bounded termination helper を呼ぶ。終了後に result file がない cancel 経路は `HumanShellLaunchError::Cancelled` として返す。

## 4. aish cleanup 方針

`terminate_pty_session` は `aish` 側の cleanup 正本にする。

必須手順:

1. PTY master を close して shell / foreground job へ EOF / HUP を届ける。
2. shell process group または session leader に SIGHUP と SIGTERM を送る。
3. `waitpid(child, WNOHANG)` で直接 child を reap する。
4. grace 期限後も残る場合は SIGKILL へ escalate する。
5. 最終 wait も期限付きにし、無期限 wait を禁止する。

保証対象は設計書 §2.1 の範囲に限る。`setsid` 脱出、double-fork daemon、nohup daemon、別 PID namespace は保証しない。

## 5. termios と runtime dir cleanup

- aish 側: 既存 `StdinTermiosGuard` の RAII を維持し、cleanup 中の early return でも drop されるようにする。
- ai 側: handoff 開始直前に親 TTY の termios を保存し、handoff 終了・cancel・error のどれでも復元する no-op compatible guard を追加する。
- runtime dir: `handoff_service.execute()` の後に手書きで cleanup するだけでは不十分。`execute()` が cancel / panic 相当の unwinding 以外で早期 return する経路、または launcher が `Cancelled` を返す経路でも必ず `cleanup_runtime_handoff_dir` に到達する scope guard を置く。

## 6. テスト計画

| 種別 | 対象 | 方針 |
|------|------|------|
| 単体 | `ai/src/adapters/outbound/human_handoff.rs`, `aish/src/adapters/outbound/pty_shell.rs` | cancel 済み flag で launcher が `Cancelled` を返す、bounded wait が SIGKILL escalation する、direct child を reap する |
| helper process 統合 | `aish/tests/0057_pty_process_cleanup_hardening.rs` | helper shell で foreground sleep、background sleep、SIGTERM ignore job を起動し、cancel 後に pid が残らないことを確認する |
| 実 PTY E2E | `ai/tests/0057_pty_process_cleanup_hardening.rs` | mock aibe + real `ai` + real `aish human-shell` + PTY を 1 本通し、`ai --collaborative --timeout` が外側 watchdog より短く non-zero 終了することを確認する |
| outer watchdog | 0057 E2E 全体 | 0055 の `wait_child_with_timeout` と同様、期限超過時は process group を SIGKILL してから panic する |

CI で不安定な実 PTY テストは skip ではなく、環境条件を明示して deterministic にする。テスト時間は既存 0055 E2E の上限（最大 60 秒）を超えない。

## 7. AC とテスト関数

`scripts/spec-acceptance.toml` 登録と一致させる。

| AC ID | テスト関数 | ファイル |
|-------|------------|----------|
| `handoff_timeout_terminates_bounded` | `handoff_timeout_terminates_bounded` | `ai/tests/0057_pty_process_cleanup_hardening.rs` |
| `external_sigint_sigterm_stops_handoff` | `external_sigint_sigterm_stops_handoff` | `ai/tests/0057_pty_process_cleanup_hardening.rs` |
| `aish_human_shell_and_pty_shell_reaped` | `aish_human_shell_and_pty_shell_reaped` | `aish/tests/0057_pty_process_cleanup_hardening.rs` |
| `foreground_and_normal_background_jobs_terminated` | `foreground_and_normal_background_jobs_terminated` | `aish/tests/0057_pty_process_cleanup_hardening.rs` |
| `sigterm_ignored_escalates_sigkill` | `sigterm_ignored_escalates_sigkill` | `aish/tests/0057_pty_process_cleanup_hardening.rs` |
| `direct_children_reaped_no_zombies` | `direct_children_reaped_no_zombies` | `aish/tests/0057_pty_process_cleanup_hardening.rs` |
| `terminal_echo_canonical_restored` | `terminal_echo_canonical_restored` | `ai/tests/0057_pty_process_cleanup_hardening.rs` |
| `runtime_handoff_dir_removed_on_abort` | `runtime_handoff_dir_removed_on_abort` | `ai/tests/0057_pty_process_cleanup_hardening.rs` |
| `normal_handoff_success_path_unchanged` | `normal_handoff_success_path_unchanged` | `ai/tests/0057_pty_process_cleanup_hardening.rs` |
| `cleanup_e2e_has_outer_watchdog` | `cleanup_e2e_has_outer_watchdog` | `ai/tests/0057_pty_process_cleanup_hardening.rs` |

## 8. Non-goals

0057 の Phase に混ぜてはならない。

- resume / durable workflow / `ai resume`
- side agent / secondary agent loop
- child Work 統合
- lease / heartbeat / reconciler
- handoff 履歴永続化
- 複数 human shell 並列実行
- 監視デーモン
- cgroup 必須化
- 正常系 handoff UX の新機能追加
- `setsid` 脱出や double-fork daemon の完全掃除保証
- aibe protocol の新しい handoff lifecycle DTO

## 9. STOP-THE-LINE 条件

次が必要になったら実装を停止し、設計書と `scripts/feature-scope.toml` の scope revision 更新、Complexity Gate 再判定、必要なら別 spec 分割を行う。

- cgroup を必須 containment として導入する
- cleanup 専用 daemon / watchdog process を追加する
- handoff 状態を永続化して crash recovery する
- lease / heartbeat / reconciler が必要になる
- 複数 human handoff の並列管理が必要になる
- `setsid` 脱出や double-fork daemon の完全掃除を保証対象に入れる
- aibe protocol に新しい handoff lifecycle DTO が必要になる
- 新しい実行主体、永続 aggregate、状態機械、外部副作用が増える

## 10. 手動検証メモ

自動テストで AC を担保する。手動検証が必要になった場合は `docs/manual/0057_pty-process-cleanup-hardening.md` を追加し、少なくとも次を記載する。

- `ai --collaborative --timeout <秒>` 中の handoff timeout 手順
- timeout 後に `ps` で `aish human-shell`、PTY shell、foreground/background job が残らないことを確認する手順
- timeout / SIGINT / SIGTERM 後に `stty -a` で `echo` と `icanon` が復元されることを確認する手順
- runtime handoff dir が削除されることを確認する手順

未実施の手動検証が残る場合は最終報告の「残リスク」に明記する。

## 11. 完了条件

1. 全 0057 AC の `pending = false`
2. 対応する Rust テストから `#[ignore]` が外れて成功
3. `./scripts/verify.sh` 成功
4. 挙動・テスト方針に触れた docs が必要なら同じ変更で更新
5. 本ファイルを `docs/done/` へ移動し、`docs/0000_spec-index.md` を実装済みに更新

## 12. 仕様との差分

なし。0057 設計書を緩めない。
