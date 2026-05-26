# 0016 — `aish shell` stdin 中継スレッド修正指示書 — 仕様ドラフト

> **出典**: Codex レビュー（2026-05-27）。`aish/src/adapters/outbound/pty_shell.rs` の PTY stdin 中継で `master` fd の close タイミングと並行 write が競合し、かつ shell 終了後に `stdin_thread.join()` がブロックしうる問題。  
> **レビュー**: Codex 仕様レビュー 1 回（2026-05-27）。初回「要修正後着手」指摘を本版に反映済み。  
> **状態**: **実装済み**（2026-05-27）

## 目的

`aish shell`（PTY 対話シェル）で、次の 2 点を解消する。

1. **`master` fd の close タイミングと並行 write の競合** — `relay_master_fd` が `File::from_raw_fd(master)` で master を所有する一方、stdin 中継スレッドにも同じ fd 番号を渡して `libc::write` しており、close タイミングとライフタイムが一致しない。
2. **終了時のハング** — shell 子プロセス終了後、`stdin_thread.join()` が `stdin.read()` のブロックで返らず、**`exit` したのに aish がプロンプトに戻らない** UX バグになりうる。

## 背景（現状）

```rust
let master_fd = master;
let stdin_thread = std::thread::spawn(move || {
    copy_stdin_to_fd(master_fd);
});
let code = relay_master_fd(master, child, self)?;
let _ = stdin_thread.join();
```

- `relay_master_fd` 内: `File::from_raw_fd(master)` — master fd の所有権を `File` に移す。
- stdin スレッド: 同じ `master` を `libc::write` で使用中 — **dup なしの共有**。
- `copy_stdin_to_fd`: `std::io::stdin()` を直読みし `stdin.read()` でブロック。子 shell 終了後も stdin が開いたままなら **join が永久に待ちうる**。
- `relay_master_fd` はループ先頭で `wait_nonblocking` を見て子終了時に return する（**0016 スコープ外**だが、終了直前の PTY 出力取りこぼし race を悪化させないことは受け入れ条件で確認する）。

## スコープ

### 対象

- `aish/src/adapters/outbound/pty_shell.rs` — FD 管理、stdin 中継、終了シーケンス
- `aish` クレート内の単体 / 統合テスト（追加可能な範囲）
- `docs/manual/aish-shell-log.md` — 終了時に aish が確実に戻ることの手動確認
- `docs/architecture.md` — PTY シェル節（必要なら 1 行）

### 対象外

- `aish exec`（非 PTY の `ProcessShell`）
- JSONL ログ形式の変更
- `ai` / `aibe` の変更
- Windows 対応（プロジェクト方針どおり Unix 専用）
- stdin 中継を完全に単一スレッド `poll` ループへ統合する大規模 refactor（**代替案として記載はするが、必須ではない**）
- **`relay_master_fd` の `wait_nonblocking` 先行による PTY 出力取りこぼし race の根本修正**（0016 では悪化させないことのみ確認。別 issue 化可）

## 確定した設計判断

### FD / shutdown の所有者

| fd / リソース | 所有者 | close 責務 |
|---------------|--------|------------|
| **`master`**（relay 用） | `relay_master_fd` 内の `File` | `File` drop（relay return 時） |
| **`stdin_master`**（`dup(master)`） | stdin 中継スレッド | スレッド終了時にスレッド内で close |
| **shutdown pipe 読み端** | stdin 中継スレッド | スレッド終了時にスレッド内で close |
| **shutdown pipe 書き端** | 親（`run_shell`） | relay 返却後、`join` 前に親が close |

**禁止**: 同一 `master` RawFd を `from_raw_fd` と stdin スレッドの `write` に同時に渡さない。

### 方針一覧

| 項目 | 方針 |
|------|------|
| **stdin 用 FD** | stdin 中継スレッドには **`libc::dup(master)`** した `stdin_master` のみ渡す。relay 側は元の `master` を `File::from_raw_fd` で所有する。 |
| **dup 失敗** | `dup` 失敗時は `InteractiveShellError` を返し、spawn 前に slave / master / 既に open した pipe を close してリソースを漏らさない。 |
| **終了シグナル** | shell 子の wait 完了（`relay_master_fd` 返却）後、親が shutdown を通知して stdin スレッドを **必ず unblock** してから `join` する。 |
| **unblock 手段** | **`pipe()` による shutdown pipe**（`eventfd` は使わない。Unix 専用スコープ内で単純さ優先）。stdin スレッドは `poll` で `stdin` と shutdown 読み端を待つ。 |
| **親の終了順序（固定）** | `relay_master_fd` return → **親が shutdown 書き端を close** → **`stdin_thread.join()`** → `Ok(code)`。この順序を `run_shell` で保証する。 |
| **`join` 必須** | スレッドは **detach しない**。必ず `join` して panic を伝播させ、fd リークを防ぐ。 |
| **TTY 前提** | 実ターミナルでの対話を主経路とする。非 TTY stdin の挙動は現状維持（ブロック / 即 EOF は環境依存）。 |
| **境界** | 変更は **aish** に閉じる。aibe / ai へ波及させない。 |

## 実装方針（推奨）

### A. FD 分離（必須）

1. `open_pty_pair` 後、親側で `stdin_master = dup(master)` を取得する。
2. stdin スレッドには `stdin_master` のみ渡す。relay には元の `master` を渡す。
3. 各経路の `File` / `close` 責務を上表どおり実装する（**同一 RawFd を二箇所に渡さない**）。

### B. stdin スレッドの unblock（必須）

1. `pipe()` で shutdown 用 pipe を作成する（`[read_fd, write_fd]`）。
2. stdin スレッドは `poll` で `[stdin, shutdown_read_fd]` を監視する（`stdin_master` は `poll` 対象に含めない。`read`/`write` はループ内で処理）。
3. 親（`run_shell`）は `relay_master_fd` が return した **後**:
   - shutdown pipe の **書き端を close** する（`poll` が読み端で unblock される）。
   - 続けて **`stdin_thread.join()`** する。
4. stdin スレッドは shutdown 通知・stdin EOF・`stdin_master` への write 失敗のいずれかでループを抜け、**自分が所有する fd**（`stdin_master`、shutdown 読み端）を close して終了する。
5. **`join` が有限時間で返る** ことを pipe ベースの単体テストで確認する。

### C. テスト seam（必須）

`std::io::stdin()` 直読みの private 関数のままでは CI（非 TTY）で終了シーケンスを deterministically に検証しにくい。次を切り出す。

- **`relay_stdin_to_pty`**（名称は実装時に調整可）: 引数に `Read` 相当（テストでは pipe 読み端）、`stdin_master: RawFd`、`shutdown_read_fd: RawFd` を取り、`poll` ループで stdin → PTY 中継する。
- **`signal_stdin_relay_shutdown`**（名称は実装時に調整可）: 親が shutdown 書き端を close する処理。`run_shell` と単体テストの両方から呼ぶ。

`#[cfg(test)]` モジュールで pipe を stdin / shutdown に差し替え、「shutdown close 後に relay スレッドが終了し join がブロックしない」ことを **自動テストの正本** とする。実 PTY E2E は手動に回す。

### D. 代替案（採用 optional）

- stdin 中継を `relay_master_fd` の `poll` ループに統合し、スレッド自体を廃止する。FD 問題は解消しやすいが diff が大きい。**時間が足りなければ A + B + C を優先**。

## 受け入れ条件

### 1. FD 分離と close 責務

- 同一 `master` RawFd を `from_raw_fd` と stdin スレッドの `write` に **同時に渡さない**（`dup` 経路を使う）。
- 上表の所有者どおり close される（コードレビュー + 単体テスト）。

### 2. shell 終了後に aish が確実に戻る

- 対話シェルで `exit`（または EOF で shell 終了）したあと、`aish shell` プロセスが **プロンプトに戻り、ハングしない**。
- 親の順序 **relay return → shutdown write close → `join`** が守られ、`stdin_thread.join()` が子終了後も永久ブロックしない。

### 3. 既存機能を壊さない

- `echo hello` → ターミナル表示 + JSONL `stdout` 記録（[aish-shell-log.md](../manual/aish-shell-log.md) の期待結果）。**出力の欠落がないこと**（0016 変更で `wait_nonblocking` 先行 race が悪化しないこと）。
- 終了コードの返却（`Ok(code)`）は従来どおり。

### 4. リソースリークなし

- 正常終了・エラー終了のいずれでも、open した fd（master, slave, dup, pipe）が close される。
- panic 経路は `Drop` または `defer` 相当で可能な限り close（Rust では `File` 所有権または明示 `close`）。

### 5. テストと docs

- pipe ベースの **単体テストを少なくとも 1 件** 追加する（C 節の seam 経由）。CI 非 TTY で通ること。
- `docs/manual/aish-shell-log.md` に「TTY 上で `exit` 後すぐ aish が終了すること（ハングしない）」を期待結果として追記する。

### 6. CI ゲート

- `cargo fmt --all -- --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `./scripts/check-architecture.sh`

## テスト方針

| 種別 | 内容 |
|------|------|
| **単体**（正本・CI） | C 節で切り出した `relay_stdin_to_pty` 等を pipe 入力で呼び、shutdown 書き端 close 後にスレッドが終了し `join` がタイムアウトしないこと。 |
| **統合** | 上記単体で足りなければ `openpty` + `sh -c 'exit 0'` の補助テスト（TTY 非依存部分のみ）。 |
| **手動**（TTY） | `docs/manual/aish-shell-log.md`: 実ターミナルで `exit` 後に即座にシェルプロンプトへ戻ること。長時間 `sleep` 入力中に別ターミナルから子を kill した場合も aish が戻ること（可能なら）。 |

**役割分担**: **CI = pipe ベース単体**。**手動 = 実ターミナル（TTY）**。非 TTY 環境での join / hang 検証は単体テストに委ね、manual は TTY 前提の注意を維持する（[aish-shell-log.md](../manual/aish-shell-log.md) 既存の非 TTY 注意と整合）。

## 実装手順の目安

1. `feature/aish-shell-stdin-fix` 等の feature ブランチを `main` から切る。
2. `dup(master)` と shutdown pipe を導入する。
3. C 節の seam に沿って stdin 中継を `poll` ベースに変更（関数名変更可）。
4. `run_shell` の終了シーケンスを **relay return → shutdown write close → join** に整理する。
5. pipe ベース単体テスト追加、`docs/manual/aish-shell-log.md` 更新。
6. fmt / clippy / test / check-architecture を通す。
7. 完了後、本ファイルを `docs/done/` へ移し `docs/0000_spec-index.md` を更新する。

## docs 更新一覧

- `docs/manual/aish-shell-log.md` — TTY 上で `exit` 後の aish 終了（ハングしないこと）
- `docs/architecture.md` — （任意）PTY stdin 中継の FD 分離・shutdown 方針
- `docs/0000_spec-index.md` — 0016 を実装済みとして `done/` に登録（完了時）

## 未確定・見送り

| 種別 | 内容 |
|------|------|
| **要確認（許容）** | 子終了直前の stdin 1 フレーム欠落 — 0016 では **許容**（shutdown 優先）。問題化したら別 issue。 |
| **見送り** | stdin 中継の単一スレッド `poll` 統合（D 案）。0016 単体では A+B+C で足りる。 |
| **見送り** | 非 TTY stdin での完全な UX 保証。 |
| **見送り** | `relay_master_fd` の `wait_nonblocking` 先行 race の根本修正（0016 スコープ外）。 |
| **推測** | 子 shell 終了で slave 側が close されれば master 読み取りは EOF になるが、**stdin スレッドの `stdin.read()` ブロック** とは独立なため、shutdown 機構が必須。 |

## 残リスク

- 実ターミナルでのみ再現する競合状態は自動テストで取りこぼす可能性がある。
- `poll` 実装のエッジケース（EINTR、部分 write）の手動確認が必要な場合がある。
- 子終了直前の stdin 1 フレームは意図的に捨てうる（上表「要確認（許容）」）。
