# 0054 — Safe File Write Tools Phase 3 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 未着手（Phase 3）  
> **前提**: [Phase 2](0054_safe-file-write-tools-phase2-implementation-spec.md) 完了

## 0. 目的

設計書 §8 の **`read_file(include_metadata=true)`** を実装する。LLM が楽観的排他制御に使う SHA-256 と line ending 情報を、既存 plain 出力を壊さずに取得できるようにする。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `include_metadata` 引数（既定 `false`） | §8 |
| metadata 行形式 `[aibe_file_metadata] {...}` | §8.1 |
| `sha256` / `size_bytes` / `line_ending` / `trailing_newline` | §8.1 |
| offset/limit は本文のみ。hash は常にファイル全体 | §8.1 |
| output truncate 時も metadata 行を先頭に残す | §8.1 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| `write_file` / `apply_patch` | 6–7 |
| 承認 UI | 8 |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `metadata_default_unchanged` | 既定は従来どおり plain text | `read_file_default_output_unchanged_without_metadata` | true |
| `metadata_includes_sha256` | `include_metadata=true` で hash 行が付く | `read_file_metadata_includes_sha256` | true |
| `metadata_hash_full_file` | offset/limit 使用時も hash は全体 | `read_file_metadata_hash_covers_full_file` | true |
| `metadata_line_endings` | lf / crlf / none / mixed を JSON に含む | `read_file_metadata_reports_line_ending` | true |
| `metadata_survives_truncate` | truncate 後も metadata 行が先頭 | `read_file_metadata_survives_output_truncate` | true |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| adapter | `aibe/src/adapters/outbound/tools/read_file.rs` |
| application | `aibe/src/application/tool_defs.rs`（引数説明） |
| tests | `aibe/tests/read_file_metadata.rs` |

## 4. 実装手順

### 4.1 引数 DTO

```json
{
  "path": "src/main.rs",
  "offset": 1,
  "limit": 200,
  "include_metadata": true
}
```

`include_metadata` 省略時は `false`。`deny_unknown_fields`。

### 4.2 出力形式（§8.1）

```text
[aibe_file_metadata] {"path":"src/main.rs","sha256":"...","size_bytes":4812,"line_ending":"lf","trailing_newline":true}
<ファイル本文>
```

- `sha256`: Phase 2 の共通関数
- `path`: JSON escape
- metadata はファイル全体を表す

### 4.3 互換性

既存クライアント・テストで `include_metadata` 未指定の出力が **1 byte も変わらない** こと。

## 5. 検証

```bash
./scripts/verify-targeted.sh --package aibe
cargo test -p aibe read_file_metadata -j 1
./scripts/verify.sh
```
