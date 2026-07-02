# 0054 — Safe File Write Tools Phase 2 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 実装済み（Phase 2）  
> **前提**: [Phase 1](0054_safe-file-write-tools-phase1-implementation-spec.md) 完了

## 0. 目的

設計書 §8.2、§11、§12.2 の **path 解決・SHA-256・テキスト検証** を共通モジュールとして実装し、**既存 `read_file` を read policy 経由に移行**する。write tool 本体・journal・diff は本 Phase では行わない。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `sha2` 依存と共通 hash 関数 | §8.2 |
| `safe_path.rs` + `ReadPathPolicy` / `WritePathPolicy` | §12.2 |
| write 用 `allowed_roots`（read とは別設定） | §12.1 |
| UTF-8 / NUL / binary 判定 | §11.1 |
| サイズ上限（`max_file_bytes` 等） | §11.2 |
| symlink / special file 拒否 | §11.3, §12.2 |
| line ending 判定（`lf` / `crlf` / `none` / `mixed`） | §8.1 |
| `read_file` の path 検証を `safe_path`（read policy）へ移行 | §12.2 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| diff / atomic write / journal | 4 |
| `read_file` metadata 出力 | 3 |
| write tool executor | 6–7 |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `write_path_allowed_roots` | write root 内の相対 path が解決される | `write_path_resolves_under_allowed_roots` | true |
| `write_path_rejects_parent` | `..` を拒否 | `write_path_rejects_parent_components` | true |
| `write_path_rejects_symlink` | target / parent symlink を拒否 | `write_path_rejects_symlinks` | true |
| `write_path_rejects_special_files` | FIFO / socket / device を拒否 | `write_path_rejects_special_files` | true |
| `read_file_uses_safe_path` | `read_file` が共通 resolver（read policy）を使う | `read_file_uses_shared_safe_path_resolver` | true |
| `read_write_roots_independent` | read roots を write に流用しない | `write_roots_are_independent_from_read_roots` | true |
| `sha256_file_hash` | ファイル全体 SHA-256（lower hex） | `sha256_hashes_file_bytes` | true |
| `text_validation_binary` | binary / invalid UTF-8 を拒否 | `text_validation_rejects_binary_and_invalid_utf8` | true |
| `line_ending_detection` | lf / crlf / none / mixed を判定 | `line_ending_detection_covers_all_kinds` | true |
| `file_size_limit` | 上限超過で `file_too_large` | `file_size_limit_enforced` | true |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| deps | `aibe/Cargo.toml`（`sha2 = "0.10"`） |
| adapter | `aibe/src/adapters/outbound/tools/safe_path.rs`（新規） |
| adapter | `aibe/src/adapters/outbound/tools/read_file.rs`（移行） |
| domain | `aibe/src/domain/file_text.rs` 等（必要なら） |
| tests | `aibe/src/adapters/outbound/tools/safe_path.rs`（unit） |
| tests | `aibe/tests/read_file_safe_path.rs` 等 |

## 4. 実装手順

### 4.1 `safe_path.rs`（§12.2）

- path は空でない、NUL なし、`..` なし
- 相対 path は `ToolExecutionContext::base_dir()` 基準
- allowed root は canonicalize して比較
- read / write で policy を分ける（許容条件が異なる）
- journal / AIBE runtime ディレクトリを特別許可しない

### 4.2 `read_file` 移行

既存の path 検証ロジックをコピーせず、`ReadPathPolicy` 経由に置換。既存 `read_file` テストが緑のままであること。

### 4.3 SHA-256（§8.2）

`read_file` / 将来の write / journal で共有する `sha256_hex(bytes)` を1か所に置く。

### 4.4 テキスト・line ending（§11, §8.1）

- NUL を含むと `binary_file_not_supported`
- `mixed` は write 時に `unsupported_line_endings`（Phase 7 で使用）

## 5. 本 Phase で返すエラー語彙（§21）

`path_not_allowed`, `symlink_not_allowed`, `unsupported_file_type`, `invalid_utf8`, `binary_file_not_supported`, `unsupported_line_endings`, `file_too_large`

## 6. 検証

```bash
./scripts/verify-targeted.sh --package aibe
./scripts/check-spec-acceptance.py
```

Phase 完了時: `./scripts/verify.sh`
