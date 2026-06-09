# 0031 — Hexagonal effect boundary 検査 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（Phase 1〜3 実装済み、`ports.no-env` 解消済み）  
> **起票**: 2026-06-08  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[0003_architecture-review-refactor-spec.md](../done/0003_architecture-review-refactor-spec.md)、[AGENTS.md](../../AGENTS.md)

## 目的

`scripts/check-hexagonal.sh` を拡張し、Ports & Adapters の **副作用 API 混入**（effect boundary 違反）を機械検査できるようにする。

現状のチェックはレイヤー間の `use` 依存方向のみを見ており、`aibe/src/application/server.rs` のように **application が inbound adapter の責務（Unix socket / filesystem / libc）を直接持つ** 違反を検出できない。本書はそのギャップを埋める設計を定義する。

## 非目標

- `check-hexagonal.sh` 本体を Bash で肥大化させること（ルールはデータ駆動とする）
- 既存の layer dependency チェックの置き換え
- クレート間依存（`check-architecture.sh`）の統合
- すべての既存違反を Phase 1 で即時修正すること（段階的に `warn` → `fail` へ昇格）
- Windows 対応

## 背景: 現状の検査が `server.rs` を見逃す理由

### 現在の `check-hexagonal.sh` が見ているもの

主に次のパターンのみ。

```text
use ...::adapters::...
```

### `server.rs` の実際の違反

`adapters` 参照だけではなく、副作用 API を直接使っている。

```rust
use tokio::net::{UnixListener, UnixStream};

std::fs::create_dir_all(...)
std::fs::remove_file(...)
std::fs::set_permissions(...)
libc::umask(...)
UnixListener::bind(...)
listener.accept().await
```

これは **application layer が inbound adapter の責務を持っている** 状態である。

### composition root 例外との組み合わせ（Phase 2 完了後）

Phase 2 で `aibe/src/application/server.rs` を adapter へ分割した後、composition root 例外は削除され `check_crate aibe` に戻した。socket / filesystem / libc の I/O は `aibe/src/adapters/inbound/unix_socket_server.rs` が担う。

## 結論: 2 層の検査

`check-hexagonal.sh` は次の 2 層に分ける。

```text
1. layer dependency rules（既存）
   - application が adapters を use していないか
   - adapters が application を use していないか
   - domain/ports が application/adapters を use していないか

2. effect boundary rules（本書で追加）
   - domain/application に std::fs, std::env, tokio::net, std::process, libc などがないか
   - socket / process / filesystem / env / HTTP / PTY などの外界 I/O が正しい adapter 側にあるか
```

現状足りないのは **2 の effect boundary rules** である。

## アーキテクチャ: ルールファイル駆動

`check-hexagonal.sh` 本体を肥大化させず、ルールファイル駆動とする。

```text
scripts/check-hexagonal.sh
  - 既存の依存方向チェック
  - 追加で rules checker を呼ぶ

scripts/hexagonal-rules.toml
  - 禁止パターン一覧

scripts/check-hexagonal-effects.py
  - rules.toml を読んで grep 的に検査

scripts/hexagonal-allowlist.toml
  - 既知の一時例外
```

### ルールファイル駆動を選ぶ理由

| 観点 | Bash 直書き | Python + TOML |
|------|-------------|---------------|
| ルール追加 | スクリプト編集が必要 | TOML に 1 エントリ追加 |
| severity / message / suggestion | 扱いにくい | 構造化しやすい |
| allowlist | 分岐が増える | 別ファイルで管理 |
| AI による拡張 | チェックロジック破壊リスク大 | データ追加で済む |
| 将来の JSON 出力 | 困難 | 容易 |

AI にルールを増やさせる前提では、**Bash ロジックを毎回触らせるより TOML にルールを 1 個追加させる** 方が安全である。

## ファイル仕様

### `scripts/hexagonal-rules.toml`

各ルールは `[[rules]]` テーブルで定義する。

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `id` | はい | 一意のルール ID（例: `application.no-unix-socket`） |
| `severity` | はい | `fail` または `warn` |
| `paths` | はい | 対象ファイルの glob（例: `*/src/application/**/*.rs`） |
| `regex` | はい | 行単位でマッチする正規表現（下記「検査方式」参照） |
| `message` | はい | 違反時の説明 |
| `suggestion` | はい | 修正方針のヒント |

#### 検査方式（line-based）

checker は **1 行ずつ** regex を適用する。rustfmt による折り返し `use` や複数行にまたがる式は、意図的に検査対象外とする（実装を単純に保つため）。

| 検出できる例 | 検出しにくい例 |
|-------------|---------------|
| `use tokio::net::UnixListener;` | 複数行 `use` の第 2 行以降のみに型名がある場合 |
| `std::fs::create_dir_all(...)` | 行末で切れたメソッドチェーンの続き行 |
| `std::env::current_dir()` | 同上 |

将来 whole-file 走査へ拡張する場合は checker ロジック変更が必要であり、ルール TOML の `regex` フィールドはそのまま流用できる。

#### 受容する false negative（Phase 1 明示）

line-based 検査では次を **意図的に検出対象外** とする。受け入れ条件に含める。

| 取りこぼし例 | 理由 |
|-------------|------|
| 複数行 `use` の第 2 行以降のみに型名がある場合 | 行単位 regex の割り切り |
| `use ... as Alias` 経由で副作用型を別名参照した場合 | 同上（Phase 2 以降で必要なら token 走査を検討） |
| 行末で切れたメソッドチェーンの続き行 | 同上 |

`aibe/src/application/server.rs` の既知違反は単行 `use` / 単行呼び出しで検出できるため、Phase 1〜2 の本命ターゲットには十分である。上記以外の構文で effect が混入した場合は、ルール追加または checker 拡張で対応する。

#### severity の意味

| 値 | 動作 |
|----|------|
| `warn` | 違反を報告するが CI は落とさない |
| `fail` | CI を落とす |

運用フロー:

```text
1. AI または人間が新しい違反パターンを発見
2. rules.toml に severity = "warn" で追加
3. 出力を見て妥当性確認
4. 既存違反を修正
5. severity = "fail" に昇格
```

いきなり `fail` だけにすると、過剰な正規表現で CI を壊しやすい。`warn → fail` の半自動化が推奨される。

### `scripts/hexagonal-allowlist.toml`

既存の技術的負債で一時的に CI を赤くしないための例外。**ファイル単位の恒久例外は禁止**とし、違反行を特定して期限付きで管理する。

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `rule` | はい | 対象ルール ID |
| `path` | はい | リポジトリルートからの相対パス |
| `line` | はい | 対象行番号（1 始まり）。同一ファイル内の別行の新規違反は allowlist で隠さない |
| `reason` | はい | 例外理由 |
| `remove_by` | はい | 削除条件。形式: `phase:N`（例: `phase:3`）または設計書番号（例: `0031-phase-3`） |

checker は `(rule, path, line)` の完全一致のみ除外する。`line` 省略エントリはメタチェックで reject する。

**重要**: 本書が検出対象とする本命違反（`aibe/src/application/server.rs`）は allowlist に入れない。先に adapter へ移すタスクとする。

#### `warn` severity の運用

`severity = "warn"` のルール（現状 `ports.no-filesystem`）は恒久化を防ぐため、昇格先 phase または修正タスクを設計書に明記する。`ports.no-env` は解消済みで `fail` に昇格した。

### `scripts/check-hexagonal-effects.py`

- `hexagonal-rules.toml` を読み、対象ファイルに regex を適用
- `hexagonal-allowlist.toml` で一致を除外
- `severity = "fail"` の違反が 1 件でもあれば exit 1
- `severity = "warn"` のみの場合は exit 0（警告は stderr に出力）

#### ルール自体のメタチェック（checker 起動時）

ルール追加の品質を保つため、checker 起動時に次を検証する。

| 検証項目 | 失敗時 |
|---------|--------|
| `id` が重複していない | checker 自体が exit 1 |
| `severity` が `fail` / `warn` のいずれか | 同上 |
| `paths` が空ではない | 同上 |
| `regex` がコンパイル可能 | 同上 |
| `message` が空ではない | 同上 |
| `suggestion` が空ではない | 同上 |
| allowlist の `line` が正の整数 | 同上 |
| allowlist の `remove_by` が空ではない | 同上 |

## 初期ルール一覧

各ルールの **Phase 1 時点の severity** は下表のとおり。Phase 3 以降の変更は「Phase ごとの severity 正本」を正とする。

### application 層（Phase 1: すべて `fail`）

| ID | 禁止パターン（regex 概要） |
|----|---------------------------|
| `application.no-unix-socket` | `tokio::net::UnixListener`, `UnixListener`, `UnixStream` 等 |
| `application.no-filesystem-io` | `std::fs::`, `File::open`, `File::create`, `OpenOptions::`, `canonicalize(`, `metadata(`, `set_permissions(`, `remove_file(`, `create_dir_all(` |
| `application.no-env` | `std::env::`（`current_dir()` 含む） |
| `application.no-process` | `std::process::`, `tokio::process::`, `Command::new` |
| `application.no-libc` | `libc::` |
| `application.no-http` | `reqwest`, `hyper`, `ureq`, `isahc`, `surf` の `::` 呼び出し |

`std::env::current_dir()` による client cwd 解決は application ではなく **composition root**（`main.rs` 等）に寄せる。現状の違反例: `ai/src/application/ask.rs`。

### domain 層（Phase 1: すべて `fail`。Phase 3 で既知違反を修正済み）

| ID | 禁止パターン（regex 概要） |
|----|---------------------------|
| `domain.no-filesystem` | `std::fs::`, `File::open`, `File::create`, `OpenOptions::`, `canonicalize(`, `metadata(` |
| `domain.no-env` | `std::env::` |
| `domain.no-process` | `std::process::`, `tokio::process::`, `Command::new` |
| `domain.no-async-runtime` | `tokio::`, `async_trait` |
| `domain.no-http` | `reqwest`, `hyper` 等 |

Phase 3 で `domain.no-env` / `domain.no-filesystem` の既知違反は修正済み。allowlist は空のまま、severity は `fail` を維持する。

### ports 層（段階的導入）

`ports` は trait 定義中心にしたい。`aibe/src/ports/outbound/config.rs` にあった `std::env::var("HOME")` は **解消済み**（`default_conversation_store_root_with_home(home)` へ純粋関数化し、`HOME` 取得は adapter 側へ移動）。`ports.no-env` は `fail` に昇格済み。

| ID | severity | 禁止パターン |
|----|----------|-------------|
| `ports.no-process` | `fail` | `std::process::`, `tokio::process::`, `Command::new` |
| `ports.no-http` | `fail` | HTTP クライアント crate の `::` 呼び出し |
| `ports.no-filesystem` | `warn` | filesystem I/O パターン（application と同系統） |
| `ports.no-env` | `fail` | `std::env::` |

## Phase ごとの severity 正本

`scripts/hexagonal-rules.toml` の severity は phase ごとに次のとおり。**同一ルール ID の severity を phase 内で混在させない。**

| ルール ID | Phase 1 | Phase 3 以降（現状） | 備考 |
|-----------|---------|----------------------|------|
| `application.*` | `fail` | `fail` | Phase 2 で `server.rs` 分割後も pass |
| `domain.*` | `fail` + allowlist | `fail`、allowlist 空 | `llm_profile.rs` / `shell_log_resolve.rs` 違反は解消済み |
| `ports.no-process` / `ports.no-http` | `fail` | `fail` | 変更なし |
| `ports.no-filesystem` | `warn` | `warn` | 将来 `fail` 昇格を検討 |
| `ports.no-env` | `warn` | **`fail`** | `config.rs` の `HOME` 参照を adapter へ移動済み |

Phase 1 の `hexagonal-rules.toml` は **application / domain / ports すべてのルールを最初から入れる**。domain の既知違反は Phase 3 で修正し、allowlist は空のまま維持する。

## ルールファイル例（Phase 1 正本）

実装時の正本は `scripts/hexagonal-rules.toml` とする。Phase 1 で投入する初期内容は次のとおり。

```toml
# scripts/hexagonal-rules.toml

[[rules]]
id = "application.no-unix-socket"
severity = "fail"
paths = ["*/src/application/*.rs", "*/src/application/**/*.rs"]
regex = "\\b(tokio::net::(UnixListener|UnixStream)|UnixListener|UnixStream)\\b"
message = "application layer must not own Unix socket I/O"
suggestion = "Move socket accept/read/write loop to adapters/inbound/unix_socket_server.rs"

[[rules]]
id = "application.no-filesystem-io"
severity = "fail"
paths = ["*/src/application/*.rs", "*/src/application/**/*.rs"]
regex = "\\b(std::fs::|File::open|File::create|OpenOptions::|canonicalize\\(|metadata\\(|set_permissions\\(|remove_file\\(|create_dir_all\\()"
message = "application layer must not perform filesystem I/O directly"
suggestion = "Move filesystem access behind an outbound port or into an adapter"

[[rules]]
id = "application.no-env"
severity = "fail"
paths = ["*/src/application/*.rs", "*/src/application/**/*.rs"]
regex = "\\bstd::env::"
message = "application layer must not read environment variables or process cwd"
suggestion = "Resolve env/cwd in composition root (main.rs) and pass values into use cases"

[[rules]]
id = "application.no-process"
severity = "fail"
paths = ["*/src/application/*.rs", "*/src/application/**/*.rs"]
regex = "\\b(std::process::|tokio::process::|Command::new)"
message = "application layer must not spawn external processes directly"
suggestion = "Use a ProcessRunner/ShellExecutor port and implement it in adapters"

[[rules]]
id = "application.no-libc"
severity = "fail"
paths = ["*/src/application/*.rs", "*/src/application/**/*.rs"]
regex = "\\blibc::"
message = "application layer must not call libc directly"
suggestion = "Move OS-specific calls to adapters"

[[rules]]
id = "application.no-http"
severity = "fail"
paths = ["*/src/application/*.rs", "*/src/application/**/*.rs"]
regex = "\\b(reqwest|hyper|ureq|isahc|surf)::"
message = "application layer must not perform HTTP directly"
suggestion = "Use an outbound port such as LlmProvider"

[[rules]]
id = "domain.no-filesystem"
severity = "fail"
paths = ["*/src/domain/*.rs", "*/src/domain/**/*.rs"]
regex = "\\b(std::fs::|File::open|File::create|OpenOptions::|canonicalize\\(|metadata\\()"
message = "domain layer must not perform filesystem I/O"
suggestion = "Keep only pure policy/value logic in domain; move I/O to adapters"

[[rules]]
id = "domain.no-env"
severity = "fail"
paths = ["*/src/domain/*.rs", "*/src/domain/**/*.rs"]
regex = "\\bstd::env::"
message = "domain layer must not read environment variables"
suggestion = "Read env in adapter/composition root and pass values into domain functions"

[[rules]]
id = "domain.no-process"
severity = "fail"
paths = ["*/src/domain/*.rs", "*/src/domain/**/*.rs"]
regex = "\\b(std::process::|tokio::process::|Command::new)"
message = "domain layer must not spawn processes"
suggestion = "Move process execution to adapters"

[[rules]]
id = "domain.no-async-runtime"
severity = "fail"
paths = ["*/src/domain/*.rs", "*/src/domain/**/*.rs"]
regex = "\\b(tokio::|async_trait)"
message = "domain layer must not depend on async runtime or adapter traits"
suggestion = "Keep domain synchronous and pure where possible"

[[rules]]
id = "ports.no-process"
severity = "fail"
paths = ["*/src/ports/*.rs", "*/src/ports/**/*.rs"]
regex = "\\b(std::process::|tokio::process::|Command::new)"
message = "ports should define contracts, not spawn processes"
suggestion = "Move implementation to adapters"

[[rules]]
id = "ports.no-http"
severity = "fail"
paths = ["*/src/ports/*.rs", "*/src/ports/**/*.rs"]
regex = "\\b(reqwest|hyper|ureq|isahc|surf)::"
message = "ports should not perform HTTP"
suggestion = "Keep HTTP client details in adapters"

[[rules]]
id = "ports.no-filesystem"
severity = "warn"
paths = ["*/src/ports/*.rs", "*/src/ports/**/*.rs"]
regex = "\\b(std::fs::|File::open|File::create|OpenOptions::|canonicalize\\(|metadata\\(|set_permissions\\()"
message = "ports should not perform filesystem I/O"
suggestion = "Keep filesystem access in adapters and pass resolved values into ports"

[[rules]]
id = "ports.no-env"
severity = "fail"
paths = ["*/src/ports/*.rs", "*/src/ports/**/*.rs"]
regex = "\\bstd::env::"
message = "ports should not read environment variables"
suggestion = "Read env in adapter/composition root and inject the resolved values"
```

### Phase 3 正本（完了）

Phase 3 では次を実施済み。

1. `ai/src/domain/llm_profile.rs` — env 読み取りを composition root へ移動
2. `ai/src/domain/shell_log_resolve.rs` — domain から削除し `domain/shell_log.rs` + `adapters/outbound/shell_log_resolver.rs` へ分離
3. `ai/src/application/ask.rs` — env 読み取りを composition root へ移動
4. `hexagonal-allowlist.toml` — 空のまま維持（違反は修正済み）

### 本ルールで検出されていた既知違反（解消済み）

| ファイル | 検出ルール | 現状 |
|---------|-----------|------|
| `aibe/src/application/server.rs` | `application.no-unix-socket`, `application.no-filesystem-io`, `application.no-libc` | **Phase 2 で修正**（`unix_socket_server.rs` へ分割） |
| `ai/src/application/ask.rs` | `application.no-env` | **Phase 3 で修正** |
| `ai/src/domain/llm_profile.rs` | `domain.no-env` | **Phase 3 で修正** |
| `ai/src/domain/shell_log_resolve.rs` | `domain.no-filesystem` | **Phase 3 で修正**（ファイル削除・責務分離） |
| `aibe/src/ports/outbound/config.rs` | `ports.no-env` | **修正済み**（`default_conversation_store_root_with_home` へ純粋関数化） |

## allowlist 正本（現状）

`scripts/hexagonal-allowlist.toml` は **空**（Phase 3 完了後も維持）。一時例外が必要な場合のみ `(rule, path, line, reason, remove_by)` 形式で追加する。

**allowlist に入れないファイル**: composition root 例外を使わず、本命違反は修正してから CI を通す（`server.rs` 分割が先例）。

Phase 1 で想定していた allowlist 例（`ask.rs` / `llm_profile.rs` / `shell_log_resolve.rs`）は、Phase 3 の修正により不要となった。

## `check-hexagonal.sh` への統合

`check-hexagonal.sh` はエントリーポイントとして残し、既存チェックの後に effect boundary checker を呼ぶ。

```bash
# effect boundary rules
if command -v python3 >/dev/null 2>&1; then
  python3 "$ROOT/scripts/check-hexagonal-effects.py"
else
  fail "python3 is required for hexagonal effect boundary checks"
fi
```

`./scripts/check-architecture.sh` → `check-hexagonal.sh` → `check-hexagonal-effects.py` の呼び出し経路は維持する。

## 出力形式

単に落とすだけでなく、AI が修正しやすい出力とする。`rule id` と `suggestion` を必ず含める。

```text
HEXAGONAL FAIL [application.no-unix-socket]
  aibe/src/application/server.rs:9
  use tokio::net::{UnixListener, UnixStream};

  application layer must not own Unix socket I/O
  Suggestion:
    Move socket accept/read/write loop to adapters/inbound/unix_socket_server.rs

HEXAGONAL FAIL [application.no-filesystem-io]
  aibe/src/application/server.rs:66
  std::fs::create_dir_all(parent)?;

  application layer must not perform filesystem I/O directly
  Suggestion:
    Move filesystem access behind an outbound port or into an adapter
```

`warn` の場合は `HEXAGONAL WARN [rule-id]` プレフィックスとする。

```text
HEXAGONAL WARN [ports.no-env]
  aibe/src/ports/outbound/config.rs:188
  let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());

  ports should not read environment variables
  Suggestion:
    Read env in adapter/composition root and inject the resolved values
```

## AI 向け運用ルール（ルール追加の作法）

新しい hexagonal boundary ルールを追加するときは次に従う。実装完了後は `AGENTS.md` および `docs/architecture.md` にも同内容を追記する。

```text
When adding a new hexagonal boundary rule:
1. Do not edit check-hexagonal.sh unless the checker itself lacks required capability.
2. Add a new [[rules]] entry to scripts/hexagonal-rules.toml.
3. Use severity = "warn" if existing violations are expected.
4. Use severity = "fail" only when the repository already passes or the same change fixes violations.
5. Include a specific suggestion.
6. Prefer narrow regex over broad regex.
7. If allowing an existing violation, add scripts/hexagonal-allowlist.toml entry with rule, path, line, reason, and remove_by.
8. Update docs/architecture.md if the rule changes the architectural contract.
9. When changing severity across phases, update the "Phase ごとの severity 正本" table in this spec.
```

これにより AI がチェックロジック本体を壊すリスクを減らす。

## 段階的実装計画

### Phase 1: effect boundary checker を追加

追加するファイル:

```text
scripts/hexagonal-rules.toml
scripts/hexagonal-allowlist.toml
scripts/check-hexagonal-effects.py
```

`check-hexagonal.sh` から呼び出す。

**投入するルール**: 本書「ルールファイル例（Phase 1 正本）」の全ルール（application 6 件 + domain 5 件 + ports 4 件 = 15 件）。

**受け入れ条件**:

- `./scripts/check-hexagonal-effects.py` 単体で `server.rs` が `application.no-unix-socket` 等で落ちること
- `ai/src/application/ask.rs` は `application.no-env` で検出されるが、行番号付き allowlist により CI は `server.rs` 分のみ fail すること
- `aibe/src/ports/outbound/config.rs` は `ports.no-env` で `HEXAGONAL WARN` として出力されること
- `./scripts/verify.sh` は Phase 1 時点では **意図的に失敗しうる**（`server.rs` 未修正のため）。checker 追加までを Phase 1 のコード完了とする
- ルールメタチェック（重複 ID、不正 severity、allowlist の `line` / `remove_by` 等）が動作すること
- **docs 同期（Phase 1 必須）**: `docs/architecture.md` に effect boundary 小節を追記、`docs/testing.md` に実行経路を追記（AGENTS.md の「機能変更時は docs を同じ変更で更新」に従う）

### Phase 2: `server.rs` を adapter へ移動

本命の構造修正。

```text
aibe/src/application/server.rs
```

を分割する。

| 移動先 | 責務 |
|--------|------|
| `aibe/src/adapters/inbound/unix_socket_server.rs` | `UnixListener`, `UnixStream`, NDJSON read/write, `ConnectionApprovalGate`, `ConnectionEventSink`, socket chmod / umask |
| `aibe/src/application/request_service.rs` | request handling のみ |
| `aibe/src/main.rs` または `composition.rs` | registry / terminator / store / service / server の組み立て |

この変更後、`check_crate aibe server.rs` の composition root 例外を削除し、`check_crate aibe` に戻す。

**受け入れ条件**:

- effect boundary checker が `aibe` application 層で pass すること
- `./scripts/verify.sh` が pass すること
- 既存の socket プロトコルテストが pass すること

### Phase 3: 既存 domain / application 違反の整理（完了）

対象は解消済み。

```text
ai/src/domain/llm_profile.rs       → env 読み取りを composition root へ
ai/src/domain/shell_log_resolve.rs → 削除（shell_log.rs + shell_log_resolver.rs へ分離）
ai/src/application/ask.rs          → env 読み取りを composition root へ
aibe/src/ports/outbound/config.rs  → default_conversation_store_root_with_home へ純粋関数化
```

**受け入れ条件**（達成済み）:

- domain / application の既知違反が effect boundary checker で pass すること
- `hexagonal-allowlist.toml` が空のまま `./scripts/verify.sh` が pass すること
- `ports.no-env` が `fail` に昇格し pass すること

### Phase 4: AI 用運用ルールの docs 同期

本書「AI 向け運用ルール」節を `AGENTS.md` に反映する。`docs/architecture.md` / `docs/testing.md` の effect boundary 記述は **Phase 1 で投入済み** とし、Phase 4 では AGENTS.md の追記と必要なら architecture の運用ルール参照リンクを整える。

## `docs/architecture.md` への追記（Phase 1 必須）

Hexagonal 節に effect boundary 検査の説明を追加する。

| 項目 | 内容 |
|------|------|
| 検査スクリプト | `scripts/check-hexagonal-effects.py` |
| ルール正本 | `scripts/hexagonal-rules.toml` |
| 例外正本 | `scripts/hexagonal-allowlist.toml` |
| severity 運用 | `warn` → 修正 → `fail` 昇格 |
| ルール追加手順 | 本書「AI 向け運用ルール」参照 |

既存の「レイヤー依存」表は維持し、その直後に「effect boundary（副作用 API）」小節を追加する。

## 受け入れ条件（全体）

1. layer dependency rules（既存）と effect boundary rules（新規）の 2 層が `./scripts/verify.sh` 経由で実行される
2. `aibe/src/application/server.rs` の socket / filesystem / libc 違反が検出される（Phase 1 時点）
3. `ai/src/application/ask.rs` の `application.no-env` 違反は Phase 3 で修正済み
4. `aibe/src/ports/outbound/config.rs` の `ports.no-env` は修正後 `fail` で pass すること
5. Phase 2 完了後、`server.rs` 分割により effect boundary が pass し、composition root 例外が不要になる
6. ルール追加は TOML データ追加で完結し、checker ロジック変更は最小限
7. 出力に `rule id`・行番号・`message`・`suggestion` が含まれる
8. Phase ごとの severity 正本が `hexagonal-rules.toml` と矛盾しない
9. `AGENTS.md` / `docs/architecture.md` / `docs/testing.md` が本設計と同期している

## 未確定

- `ports.no-filesystem` を `fail` に昇格するタイミング（現状 `warn` のまま）
- PTY 関連（`aish` の `pty_shell.rs` 等）を effect boundary ルールに含めるかは将来検討（初期ルールには含めない）
- line-based 検査で multiline 構文の false negative が実害になるかは、checker 導入後の運用で判断（現時点は割り切り）
- import 系ルール（`domain.no-std-fs-import` 等）の追加は別タスク（P1）
