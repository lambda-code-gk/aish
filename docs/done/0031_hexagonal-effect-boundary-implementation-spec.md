# 0031 — Hexagonal effect boundary 実装指示書

> **種別**: 実装済み指示書（`docs/done/`）  
> **状態**: 実装済み  
> **設計の正本**: [0031_hexagonal-effect-boundary-spec.md](../spec/0031_hexagonal-effect-boundary-spec.md)  
> **完了確認**: `./scripts/verify.sh` / `./scripts/smoke-mock.sh`  
> **起票**: 2026-06-08  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[AGENTS.md](../../AGENTS.md)、[0003_architecture-review-refactor-spec.md](../done/0003_architecture-review-refactor-spec.md)

## 目的

`scripts/check-hexagonal.sh` に **effect boundary 検査** を追加し、Ports & Adapters の副作用 API 混入を機械検査できる状態にする。  
本指示書は設計書 `docs/spec/0031_hexagonal-effect-boundary-spec.md` の Phase 1〜4 をそのまま実装計画に落としたもので、実装は **checker 追加 → `server.rs` 分割 → 既知違反整理 → docs 同期** の順で進める。

最終到達点は次の 3 点。

1. `scripts/check-hexagonal-effects.py` が `scripts/hexagonal-rules.toml` と `scripts/hexagonal-allowlist.toml` を読み、`application` / `domain` / `ports` の副作用違反を検出する
2. `aibe/src/application/server.rs` の socket / filesystem / libc 責務を `aibe/src/adapters/inbound/unix_socket_server.rs` へ移す
3. `./scripts/verify.sh` と `./scripts/check-docs-consistency.sh` が本指示書どおりの更新後に通る

## 実装範囲

- Phase 1: effect boundary checker の追加
- Phase 2: `aibe/src/application/server.rs` の分割と composition root 化
- Phase 3: 既知違反の allowlist / severity 整理
- Phase 4: `docs/` 同期と検証経路の固定

## 実装順

1. `scripts/hexagonal-rules.toml` と `scripts/hexagonal-allowlist.toml` を正本として作る
2. `scripts/check-hexagonal-effects.py` を追加する
3. `scripts/check-hexagonal.sh` から Python checker を呼ぶ
4. `aibe/src/application/server.rs` の責務を分割し、socket server を adapter 側へ移す
5. 既知違反の allowlist を Phase 3 までの前提で整理する
6. `docs/architecture.md` と `docs/testing.md` を同時に更新する

## ファイル別の作業手順

| パス | 作業 |
|------|------|
| `scripts/check-hexagonal.sh` | 既存の layer dependency チェックは維持し、後段で `scripts/check-hexagonal-effects.py` を呼ぶ。Phase 2 完了後は `check_crate aibe server.rs` の composition root 例外を外し、`check_crate aibe` に戻す |
| `scripts/check-hexagonal-effects.py` | 新規作成。`hexagonal-rules.toml` / `hexagonal-allowlist.toml` を読み、対象 `.rs` に line-based regex を適用する |
| `scripts/hexagonal-rules.toml` | 正本。`application` / `domain` / `ports` の副作用禁止ルールを列挙する。Phase 1 で全ルールを投入する |
| `scripts/hexagonal-allowlist.toml` | 正本。既知の一時例外のみを行番号付きで列挙する。`server.rs` は allowlist に入れない |
| `aibe/src/application/server.rs` | Phase 2 で composition root に寄せる。socket path 準備・bind・accept loop・NDJSON 送受信・approval gate・event sink を adapter へ移す |
| `aibe/src/adapters/inbound/unix_socket_server.rs` | Phase 2 で新規作成。Unix socket サーバの実際の I/O と接続ループを保持する |
| `aibe/src/application/request_service.rs` | 変更最小限。request orchestration のみ保持し、socket / filesystem / libc への依存を持たない |
| `docs/architecture.md` | Hexagonal 節に effect boundary の説明、ルール正本、運用ルールを追記する |
| `docs/testing.md` | `check-hexagonal-effects.py` の役割、Phase 別の検証コマンド、テスト配置を追記する |

## allowlist 正本（行番号付き）

`scripts/hexagonal-allowlist.toml` の正本は次のとおり。**line は現時点の source line をそのまま使う**。checker は `(rule, path, line)` 完全一致のみ除外し、ファイル単位の例外は認めない。

```toml
# scripts/hexagonal-allowlist.toml

[[allow]]
rule = "application.no-env"
path = "ai/src/application/ask.rs"
line = 77
reason = "client cwd resolution is still in Ask run path; move it to composition root"
remove_by = "phase:3"

[[allow]]
rule = "domain.no-env"
path = "ai/src/domain/llm_profile.rs"
line = 10
reason = "existing env lookup in domain; move AI_LLM_PROFILE resolution to CLI/composition root"
remove_by = "phase:3"

[[allow]]
rule = "domain.no-filesystem"
path = "ai/src/domain/shell_log_resolve.rs"
line = 69
reason = "existing canonicalize call in domain; move path resolution to adapter or composition root"
remove_by = "phase:3"

[[allow]]
rule = "domain.no-filesystem"
path = "ai/src/domain/shell_log_resolve.rs"
line = 105
reason = "existing canonicalize call in domain; move path resolution to adapter or composition root"
remove_by = "phase:3"

[[allow]]
rule = "domain.no-filesystem"
path = "ai/src/domain/shell_log_resolve.rs"
line = 110
reason = "existing metadata read in domain; move filesystem access out of domain"
remove_by = "phase:3"

[[allow]]
rule = "domain.no-filesystem"
path = "ai/src/domain/shell_log_resolve.rs"
line = 134
reason = "existing file open in domain; move filesystem access out of domain"
remove_by = "phase:3"
```

### allowlist の運用ルール

- `aibe/src/application/server.rs` は allowlist に入れない
- `ports.no-env` / `ports.no-filesystem` は warn 出力で可視化するため、allowlist で隠さない
- allowlist を追加するときは `remove_by` を必須にする
- 行番号がずれたら、checker 出力に合わせて同じ PR で更新する

## `hexagonal-rules.toml` 正本

Phase 1 の正本は設計書からそのまま転記してよい。以下を `scripts/hexagonal-rules.toml` の初期内容とする。

```toml
# scripts/hexagonal-rules.toml

[[rules]]
id = "application.no-unix-socket"
severity = "fail"
paths = ["*/src/application/**/*.rs"]
regex = "\\b(tokio::net::(UnixListener|UnixStream)|UnixListener|UnixStream)\\b"
message = "application layer must not own Unix socket I/O"
suggestion = "Move socket accept/read/write loop to adapters/inbound/unix_socket_server.rs"

[[rules]]
id = "application.no-filesystem-io"
severity = "fail"
paths = ["*/src/application/**/*.rs"]
regex = "\\b(std::fs::|File::open|File::create|OpenOptions::|canonicalize\\(|metadata\\(|set_permissions\\(|remove_file\\(|create_dir_all\\()"
message = "application layer must not perform filesystem I/O directly"
suggestion = "Move filesystem access behind an outbound port or into an adapter"

[[rules]]
id = "application.no-env"
severity = "fail"
paths = ["*/src/application/**/*.rs"]
regex = "\\bstd::env::"
message = "application layer must not read environment variables or process cwd"
suggestion = "Resolve env/cwd in composition root (main.rs) and pass values into use cases"

[[rules]]
id = "application.no-process"
severity = "fail"
paths = ["*/src/application/**/*.rs"]
regex = "\\b(std::process::|tokio::process::|Command::new)"
message = "application layer must not spawn external processes directly"
suggestion = "Use a ProcessRunner/ShellExecutor port and implement it in adapters"

[[rules]]
id = "application.no-libc"
severity = "fail"
paths = ["*/src/application/**/*.rs"]
regex = "\\blibc::"
message = "application layer must not call libc directly"
suggestion = "Move OS-specific calls to adapters"

[[rules]]
id = "application.no-http"
severity = "fail"
paths = ["*/src/application/**/*.rs"]
regex = "\\b(reqwest|hyper|ureq|isahc|surf)::"
message = "application layer must not perform HTTP directly"
suggestion = "Use an outbound port such as LlmProvider"

[[rules]]
id = "domain.no-filesystem"
severity = "fail"
paths = ["*/src/domain/**/*.rs"]
regex = "\\b(std::fs::|File::open|File::create|OpenOptions::|canonicalize\\(|metadata\\()"
message = "domain layer must not perform filesystem I/O"
suggestion = "Keep only pure policy/value logic in domain; move I/O to adapters"

[[rules]]
id = "domain.no-env"
severity = "fail"
paths = ["*/src/domain/**/*.rs"]
regex = "\\bstd::env::"
message = "domain layer must not read environment variables"
suggestion = "Read env in adapter/composition root and pass values into domain functions"

[[rules]]
id = "domain.no-process"
severity = "fail"
paths = ["*/src/domain/**/*.rs"]
regex = "\\b(std::process::|tokio::process::|Command::new)"
message = "domain layer must not spawn processes"
suggestion = "Move process execution to adapters"

[[rules]]
id = "domain.no-async-runtime"
severity = "fail"
paths = ["*/src/domain/**/*.rs"]
regex = "\\b(tokio::|async_trait)"
message = "domain layer must not depend on async runtime or adapter traits"
suggestion = "Keep domain synchronous and pure where possible"

[[rules]]
id = "domain.no-http"
severity = "fail"
paths = ["*/src/domain/**/*.rs"]
regex = "\\b(reqwest|hyper)::"
message = "domain layer must not perform HTTP directly"
suggestion = "Keep HTTP client details in adapters"

[[rules]]
id = "ports.no-process"
severity = "fail"
paths = ["*/src/ports/**/*.rs"]
regex = "\\b(std::process::|tokio::process::|Command::new)"
message = "ports should define contracts, not spawn processes"
suggestion = "Move implementation to adapters"

[[rules]]
id = "ports.no-http"
severity = "fail"
paths = ["*/src/ports/**/*.rs"]
regex = "\\b(reqwest|hyper|ureq|isahc|surf)::"
message = "ports should not perform HTTP"
suggestion = "Keep HTTP client details in adapters"

[[rules]]
id = "ports.no-filesystem"
severity = "warn"
paths = ["*/src/ports/**/*.rs"]
regex = "\\b(std::fs::|File::open|File::create|OpenOptions::|canonicalize\\(|metadata\\(|set_permissions\\()"
message = "ports should not perform filesystem I/O"
suggestion = "Keep filesystem access in adapters and pass resolved values into ports"

[[rules]]
id = "ports.no-env"
severity = "warn"
paths = ["*/src/ports/**/*.rs"]
regex = "\\bstd::env::"
message = "ports should not read environment variables"
suggestion = "Read env in adapter/composition root and inject the resolved values"
```

### Phase ごとの severity 正本

| ルール ID | Phase 1 | Phase 3 以降 | 備考 |
|-----------|---------|--------------|------|
| `application.*` | `fail` | `fail` | 変更なし |
| `domain.no-env` | `fail` + allowlist | `warn`、allowlist 削除 | Phase 3 で可視化する |
| `domain.no-filesystem` | `fail` + allowlist | `warn`、allowlist 削除 | 同上 |
| `domain.no-process` / `domain.no-async-runtime` / `domain.no-http` | `fail` | `fail` | 変更なし |
| `ports.no-process` / `ports.no-http` | `fail` | `fail` | 変更なし |
| `ports.no-filesystem` / `ports.no-env` | `warn` | `warn` のまま | `config.rs` の責務整理が別途必要 |

## `check-hexagonal-effects.py` の要件

### CLI

- 既定の実行形は `python3 scripts/check-hexagonal-effects.py`
- 省略時は `scripts/hexagonal-rules.toml` と `scripts/hexagonal-allowlist.toml` を読む
- テスト容易性のため、`--rules`, `--allowlist`, `--root` は任意で受けてよい
- ルールファイル / allowlist が存在しない場合は失敗として扱う

### 入出力

- 出力は stderr のみを正とする
- `warn` と `fail` はどちらも人間が読める text 形式で出す
- 1 件ずつ `HEXAGONAL WARN [rule-id]` または `HEXAGONAL FAIL [rule-id]` の見出しを出す
- 各違反は `path:line`、該当行、`message`、`Suggestion:` を含める
- warn のみなら exit 0、fail が 1 件でもあれば exit 1

### メタチェック

checker 起動時に次を検証し、壊れたルール定義を先に落とす。

| 検証項目 | 失敗時の扱い |
|---------|--------------|
| `id` の重複なし | exit 1 |
| `severity` が `fail` / `warn` のみ | exit 1 |
| `paths` が空でない | exit 1 |
| `regex` がコンパイル可能 | exit 1 |
| `message` が空でない | exit 1 |
| `suggestion` が空でない | exit 1 |
| allowlist の `line` が正の整数 | exit 1 |
| allowlist の `remove_by` が空でない | exit 1 |

### 検査方式

- line-based で 1 行ずつ regex を適用する
- 複数行にまたがる `use` やメソッドチェーンは意図的に対象外とする
- 既知違反の検出は単行 `use` / 単行呼び出しを前提にする
- 取りこぼしを増やしたくない場合のみ、ルール追加か checker 拡張で対応する

## Phase 1: checker 追加

### 実装手順

1. `scripts/hexagonal-rules.toml` と `scripts/hexagonal-allowlist.toml` を追加する
2. `scripts/check-hexagonal-effects.py` を追加する
3. TOML 解析、メタチェック、対象ファイル列挙、line-based regex 照合、allowlist 除外を実装する
4. `scripts/check-hexagonal.sh` から Python checker を呼ぶ
5. `docs/architecture.md` に effect boundary 小節を追記する
6. `docs/testing.md` に検証フローを追記する

### Phase 1 の受け入れ条件

- `./scripts/check-hexagonal-effects.py` 単体で `aibe/src/application/server.rs` の socket / filesystem / libc 違反を検出する
- `ai/src/application/ask.rs` の `application.no-env` は検出されるが、行番号付き allowlist により Phase 1 の CI では `server.rs` 分のみ fail する
- `aibe/src/ports/outbound/config.rs` の `std::env::var("HOME")` は `ports.no-env` の warn として出力される
- ルールメタチェックが壊れた TOML を正しく reject する
- `./scripts/verify.sh` は Phase 1 時点では `server.rs` 未修正のため失敗しうる

### Phase 1 の検証コマンド

```bash
python3 scripts/check-hexagonal-effects.py
./scripts/check-hexagonal.sh
./scripts/verify.sh
```

## Phase 2: `server.rs` 分割

### 対象

- `aibe/src/application/server.rs`
- `aibe/src/adapters/inbound/unix_socket_server.rs`（新規）
- 必要なら `aibe/src/adapters/inbound/mod.rs`
- `scripts/check-hexagonal.sh`

### 具体手順

1. `aibe/src/adapters/inbound/unix_socket_server.rs` を新規作成する
2. `prepare_socket_path`, `bind_unix_listener`, `serve_connection`, `write_response_line`, `ConnectionEventSink`, `tokio::net::{UnixListener, UnixStream}` を adapter 側へ移す
3. `std::fs::{create_dir_all, remove_file, set_permissions}`, `std::os::unix::fs::PermissionsExt`, `libc::umask` を application から完全に外す
4. `server.rs` は composition root に寄せ、registry / terminator / conversation store / request service を組み立てた後に adapter の socket server を呼ぶだけにする
5. `server.rs` から `UnixListener` / `UnixStream` / `BufReader` / `Mutex` / `PermissionsExt` / `libc` 依存を消す
6. `scripts/check-hexagonal.sh` の `check_crate aibe server.rs` を削除し、通常の `check_crate aibe` に戻す
7. 既存の socket / protocol テストを見直し、新しい adapter 位置へ追従させる

### Phase 2 の受け入れ条件

- `aibe/src/application/server.rs` に effect boundary 違反が残らない
- `aibe/src/adapters/inbound/unix_socket_server.rs` が socket I/O の実装正本になる
- `check-hexagonal.sh` から composition root 例外を外しても通る
- `./scripts/verify.sh` が通る
- socket / NDJSON / approval / event sink の既存契約が壊れない

### Phase 2 の検証コマンド

```bash
cargo test -p aibe --tests
cargo test -p aibe-client --tests
./scripts/check-architecture.sh
./scripts/verify.sh
```

## Phase 3: 既知違反の整理

### 対象

- `ai/src/application/ask.rs`
- `ai/src/domain/llm_profile.rs`
- `ai/src/domain/shell_log_resolve.rs`
- `scripts/hexagonal-allowlist.toml`
- `scripts/hexagonal-rules.toml`

### 実装手順

1. `ai/src/application/ask.rs` の `std::env::current_dir()` を application 外へ移す
2. `ai/src/domain/llm_profile.rs` の `std::env::var("AI_LLM_PROFILE")` を CLI / composition root 側へ移す
3. `ai/src/domain/shell_log_resolve.rs` の filesystem access を domain から外す
4. `hexagonal-allowlist.toml` から `ai` の既知違反を削除する
5. `hexagonal-rules.toml` の `domain.no-env` / `domain.no-filesystem` を `warn` に変更し、allowlist 無しで可視化する
6. 必要なら `ports.no-filesystem` / `ports.no-env` の扱いを別タスクで整理する

### Phase 3 の受け入れ条件

- `domain.no-env` と `domain.no-filesystem` は allowlist なしでも warn として見える
- `ai/src/application/ask.rs` の env 参照が残っていれば fail する
- allowlist から削除した違反が再発した場合は即 fail する

### Phase 3 の検証コマンド

```bash
python3 scripts/check-hexagonal-effects.py
cargo test -p ai --tests
./scripts/check-architecture.sh
./scripts/verify.sh
```

## Phase 4: docs 同期

### 更新対象

- `docs/architecture.md`
- `docs/testing.md`

### 追記内容

- `docs/architecture.md`
  - effect boundary 検査の目的
  - `scripts/check-hexagonal-effects.py` の位置づけ
  - `scripts/hexagonal-rules.toml` と `scripts/hexagonal-allowlist.toml` の責務
  - `warn → 修正 → fail` の運用方針
  - `server.rs` 分割後の composition root ルール
- `docs/testing.md`
  - `check-hexagonal.sh` と `check-hexagonal-effects.py` の検証経路
  - Phase ごとの検証コマンド
  - `aibe` / `ai` / `docs` の責務分担

### Phase 4 の受け入れ条件

- docs の記述が実装と矛盾しない
- `./scripts/check-docs-consistency.sh` が通る
- `./scripts/verify.sh` が通る

### Phase 4 の検証コマンド

```bash
./scripts/check-docs-consistency.sh
./scripts/verify.sh
```

## `server.rs` 分割の詳細

`aibe/src/application/server.rs` は、最終的には「組み立て」と「adapter 呼び出し」だけを持つ。  
socket I/O の実装は `aibe/src/adapters/inbound/unix_socket_server.rs` に移し、application 層は request service と ports の配線だけを担当する。

### 分割前の責務

- socket path の prepare
- UnixListener の bind
- accept loop
- UnixStream の read/write
- approval prompt 往復
- cancellation state の生成
- progress / assistant streaming の event sink
- filesystem / libc の直接利用

### 分割後の責務

- `application/server.rs`
  - registry / terminator / conversation store / request service の組み立て
  - adapter の `run(...)` 呼び出し
- `adapters/inbound/unix_socket_server.rs`
  - Unix socket の bind / accept
  - JSON line framing
  - approval gate
  - event sink
  - socket まわりの OS 依存処理

### 変更後に削除する依存

- `tokio::net`
- `std::fs`
- `std::os::unix::fs::PermissionsExt`
- `libc`
- `BufReader` / `AsyncBufReadExt` / `AsyncWriteExt` の application 側利用

## 未確定・リスク

- `check-hexagonal-effects.py` の CLI 引数をどこまで固定するかは、実装時に最小限へ寄せる余地がある
- line-based 検査なので、複数行 `use` や複数行メソッドチェーンの取りこぼしは残る
- `ai/src/domain/shell_log_resolve.rs` の filesystem logic は allowlist で可視化する前提だが、Phase 3 での移動先は別途実装が必要
- `ports.no-filesystem` / `ports.no-env` を `fail` に昇格するタイミングは `aibe/src/ports/outbound/config.rs` の責務整理と連動する
- `server.rs` 分割時に socket / approval / event sink の wire 契約を壊すと、`aibe-client` 側の統合テストに波及する

## 実装時の禁止事項

- Rust 実装コード本体をこの指示書の作成段階で変更しない
- `scripts/check-hexagonal.sh` を Bash ロジックの肥大化で解決しない
- allowlist をファイル単位で曖昧にしない
- `aibe/src/application/server.rs` を composition root 以外の役割に戻さない
- `docs/` の追記を実装と別コミットに分けない
- `git commit` / `git push` は行わない

## 最終到達点

本指示書の完了条件は次の順で満たすこと。

1. Phase 1 の checker が追加される
2. Phase 2 の `server.rs` 分割が完了する
3. Phase 3 の既知違反整理が完了する
4. Phase 4 の docs 同期が完了する
5. `./scripts/verify.sh` が通る
