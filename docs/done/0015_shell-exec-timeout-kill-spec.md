# 0015 — `shell_exec` タイムアウト時の子プロセス kill 指示書 — 仕様ドラフト

> **出典**: Codex レビュー（2026-05-27）。`aibe/src/adapters/outbound/tools/shell_exec.rs` の `timeout` 実装で spawn 済み子プロセスを明示 kill していない問題。  
> **レビュー**: Codex 仕様レビュー 2 回（2026-05-27）。初回「要修正」指摘を本版に反映済み。  
> **状態**: **実装済み**（2026-05-27）

## 目的

`shell_exec` がタイムアウトしたとき、aibe は tool result として `timeout` エラーを返すが、**OS 上の子プロセス（zombie / orphan）が残りうる**状態を解消する。

`sleep 9999` や長時間コマンドを `shell_exec` した場合、エージェント turn は終了しても子プロセスが残り、リソース漏洩・意図しないバックグラウンド実行につながる。`shell_exec` を有効にするアプリでは **P1 以前に修正する**。

## 背景（現状）

```rust
let run = async {
    let child = cmd.spawn().map_err(|e| format!("failed to spawn: {e}"))?;
    child.wait_with_output().await
        .map_err(|e| format!("failed to run command: {e}"))
};

match timeout(duration, run).await {
    // ...
    Err(_) => { /* timeout エラーを返すのみ。child は kill しない */ }
}
```

問題は 2 点ある。

1. `tokio::time::timeout` は future を打ち切るが、**spawn 済みプロセスへの kill / reap は行わない**。
2. **`timeout(duration, child.wait_with_output())` は API 上成立しない**。`wait_with_output(self)` は `Child` を move して消費するため、timeout 分岐から同じ `child` に対して `start_kill()` / `wait()` を呼べない（Codex レビュー指摘・確定）。

## スコープ

### 対象

- `aibe/src/adapters/outbound/tools/shell_exec.rs` — spawn / timeout / kill / reap の制御
- `aibe/src/adapters/outbound/tools/shell_exec.rs` 内の **単体テスト**（kill/reap 検証の正本）
- `aibe/tests/agent_turn_loop.rs` の `shell_exec_timeout_returns_tool_result_and_continues`（既存契約の維持）
- `docs/security.md` — `shell_exec` のタイムアウト後処理
- `docs/testing.md` — タイムアウト kill のテスト方針
- `docs/architecture.md` — 必要なら `shell_exec` 節に 1 行追記

### 対象外

- `exec_timeout_ms` のデフォルト値変更
- `CommandPolicy` / allowlist の拡張
- `ai` / `aish` の挙動変更
- wire protocol 変更
- プロセスグループ全体への kill（`setpgid` 等）。直接 spawn した **1 プロセス** の kill/reap のみ（孫プロセスは下記「見送り」参照）

## 確定した設計判断

| 項目 | 方針 |
|------|------|
| **Child の保持** | `spawn` 後、`mut child` を **timeout の外側**（同一 async fn 内）で保持する。timeout future に `child` を move しない。 |
| **待ち合わせ API** | **`wait_with_output` は使わない**（move するため timeout 後に child を触れない）。`child.stdout.take()` / `stderr.take()` で pipe を取得し、`timeout(duration, child.wait())` で終了待ち。正常終了後に pipe から読み取る。 |
| **timeout 時（必須）** | `child.start_kill()` → `child.wait().await` を **必ず** 実行する（best-effort で kill 失敗は無視。reap は省略しない）。tool result の error 契約は変えない。 |
| **正常終了時** | `wait()` 成功後、取得済み stdout/stderr pipe から読み取り、従来どおり `exit_code` / stdout / stderr を整形して返す。 |
| **`kill_on_drop`** | `cmd.kill_on_drop(true)` を設定するが、**補助的な保険** に過ぎない。timeout パスでの明示 `start_kill()` + `wait()` の **代替ではない**。drop 任せの実装は禁止。 |
| **tokio reap** | tokio runtime の best-effort reap に依存しない。Unix では zombie 回避のため **明示 `wait()` 成功** が必要（tokio docs 準拠）。 |
| **エラー契約** | timeout 時: `ExecutedToolStatus::Error`、`error: "timeout"`、content に `command timed out after {timeout_ms}ms`（現状維持）。 |
| **出力** | timeout 後は stdout/stderr の部分出力を **返さない**（timeout エラーのみ）。部分キャプチャは別タスク。 |
| **境界** | 変更は **aibe の `shell_exec` アダプタ** に閉じる。 |

## 実装方針（推奨）

### 禁止パターン

```rust
// NG: timeout 後に child を kill/reap できない
timeout(duration, child.wait_with_output()).await
```

### 推奨フロー

1. `Command::new(...)` 構築後、`kill_on_drop(true)` を設定する。
2. `let mut child = cmd.spawn()?` — **timeout の外**。
3. `let mut stdout = child.stdout.take()`、`let mut stderr = child.stderr.take()`（piped 前提）。
4. `match timeout(duration, child.wait()).await { ... }`
5. **正常終了** (`Ok(Ok(status))`):
   - stdout/stderr pipe を `read_to_end` 等で読み取る。
   - 従来どおり tool result を返す。
6. **timeout** (`Err(_)`):
   - `let _ = child.start_kill();`
   - `let _ = child.wait().await;` — **zombie 防止のため必須**
   - stdout/stderr の読み取り task があれば drop / abort する。
   - 既存の timeout tool result を返す。
7. spawn 失敗・`wait` 失敗は既存の `execution_failed` 経路を維持する。

**補足**: `wait()` と `wait_with_output()` を同一 `Child` に混在させない。本変更後は `wait_with_output` は使用しない。

## 受け入れ条件

### 1. タイムアウト後に子プロセスが reap される

- `sleep` 等の長時間コマンドを `timeout_ms` より短い値で `shell_exec` したとき、tool result は `timeout` エラーになる（**既存統合テスト維持**）。
- **PID ベースで reap を検証する**（「プロセス一覧に残らない」という曖昧な表現は使わない）:
  - `spawn` 直後に `child.id()` で PID を記録する。
  - `execute().await` 完了後、その PID に対し **`kill(pid, 0)` が `ESRCH` を返す**、または Linux CI では **`/proc/<pid>` が存在しない** ことを確認する。
  - 検証は **`shell_exec.rs` の単体テストを正本** とする（下記「テスト方針」）。

### 2. 正常終了・非ゼロ終了は従来どおり

- 短時間で終わるコマンドの成功 / 非ゼロ終了の tool result 契約を壊さない。
- `aibe/tests/agent_turn_loop.rs` の関連テストがすべて通る。

### 3. クレート境界を守る

- 変更は aibe 内に留まる。`ai` / `aish` への依存追加なし。
- `./scripts/check-architecture.sh` を通す。

### 4. docs を同期する

- `docs/security.md` に「タイムアウト時は子プロセスを kill して **明示 wait で reap** する」旨を追記する。
- `docs/testing.md` にタイムアウト kill/reap の **単体テスト** があることを追記する。

### 5. CI ゲート

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`

## テスト方針

| 種別 | 内容 | 優先度 |
|------|------|--------|
| **単体（正本）** | `ShellExecTool::execute` に直接 `sleep` + 短い `timeout_ms`（例: 5000ms コマンド / 100ms timeout）を渡す。spawn 後 PID を `Child::id()` で記録し、`await` 後に `ESRCH` または `/proc/<pid>` 不存在を断言する。 | **必須** |
| **統合** | `agent_turn_loop.rs` の `shell_exec_timeout_returns_tool_result_and_continues` は **timeout 結果の契約維持** のみ（status/error）。PID 検証は単体テストに委ねる。 | 既存維持 |
| **手動** | 不要（単体テストで足りる想定）。 | — |

### テスト seam（確定）

spawn 直後の PID 観測は **`run_subprocess` 内部ヘルパー分離** で行う。

- `shell_exec.rs` 内に `run_subprocess(cmd, duration) -> ShellRunOutcome` を切り出す。
- `ShellRunOutcome::TimedOut { child_pid }` に spawn 直後の `child.id()` を含める。
- `ShellExecTool::execute` はこのヘルパーを呼ぶだけにする。
- 単体テストは `mod tests` から `super::run_subprocess` を **直接呼び出し**、`TimedOut { child_pid }` 取得後に reap 検証する（`execute` 経由では PID を取らない）。

### テスト実装の注意

- **allowlist**: テスト fixture で `allowed_commands: ["sleep"]` を設定する（既存 timeout 統合テストと同様）。CI 全体の config に依存しない。
- **PID 再利用**: `kill(pid, 0)` は `wait()` 完了直後の短いウィンドウで使う。より確実なのは Linux 上の `/proc/<pid>` 不存在確認。
- **用語**: 「orphan」（親なしで実行中）と「zombie」（終了済みで reap 待ち）を混同しない。本タスクの検証対象は **いずれも残存しないこと**（=reap 済みで PID が無効）。

## 実装手順の目安

1. `feature/shell-exec-timeout-kill` 等の feature ブランチを `main` から切る。
2. `shell_exec.rs` を「推奨フロー」どおり refactor する（`wait_with_output` 廃止）。
3. `shell_exec.rs` に PID ベースの reap 単体テストを追加する。
4. `docs/security.md` / `docs/testing.md`（必要なら `architecture.md`）を更新する。
5. fmt / clippy / test / check-architecture を通す。
6. 完了後、本ファイルを `docs/done/` へ移し `docs/0000_spec-index.md` を更新する。

## docs 更新一覧

- `docs/security.md` — `shell_exec` タイムアウト時の kill + 明示 reap（`kill_on_drop` は補助のみ）
- `docs/testing.md` — タイムアウト kill/reap **単体テスト**（`shell_exec.rs`）の記述
- `docs/architecture.md` — （任意）組み込みツール表にタイムアウト後 kill の 1 行
- `docs/0000_spec-index.md` — 0015 を実装済みとして `done/` に登録（完了時）

## 未確定・見送り

| 種別 | 内容 |
|------|------|
| **見送り** | プロセスグループ（`setpgid`）単位の kill。`sh -c 'sleep 9999'` のように shell 経由だと shell のみ kill され孫が残る可能性がある。直接 `sleep` spawn（現行 `shell_exec`）を前提とする。 |
| **見送り** | timeout 時の部分 stdout/stderr の返却。 |
| **見送り** | `exec_timeout_ms` の設定 UI / CLI 変更。 |
| **確定** | `kill_on_drop(true)` だけでは timeout パスで reap が保証されない。明示 `start_kill()` + `wait()` を **必須** とする。 |

## 残リスク

- `start_kill` / `wait` が失敗した極端なケース（権限・カーネル状態）では orphan が残る可能性はゼロではない。
- 孫プロセス（shell ラッパ経由等）までは追跡しない（スコープ外）。
- 手動で `aibe` を長時間稼働させ、大量 timeout を繰り返した負荷試験は本指示書のスコープ外。
