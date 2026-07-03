# 0054 — Safe File Write Tools Phase 7 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 実装済み（Phase 7）  
> **前提**: [Phase 6](0054_safe-file-write-tools-phase6-implementation-spec.md) 完了

## 0. 目的

設計書 §10 の **`apply_patch`** を実装する。strict unified hunk parser（pure Rust、fuzzy なし）を追加し、Phase 5–6 の `FileChangeService` / `file_change_common` に接続する。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| strict single-file unified hunk parser | §10.3–10.5 |
| `expected_sha256` 必須 | §10.2 |
| context 完全一致 / hunk 順序・非重複 | §10.4 |
| `\ No newline at end of file` | §10.4 |
| CRLF 維持 / mixed 拒否 | §10.4 |
| `no_change`（結果が同一） | §10.4 |
| `patch` / `input` サイズ上限 | §11.2 |
| tool_defs LLM description | §10 |
| sanitized args（`patch_bytes`, `hunk_count`） | §20.1 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| GNU patch 互換 header（`--- a/` 行等）の受理 | 非目標 §2 |
| `ai` 承認 UI | 8 |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `apply_patch_single_hunk` | 1 hunk 成功 | `apply_patch_single_hunk_succeeds` | false |
| `apply_patch_multiple_hunks` | 複数 hunk 成功 | `apply_patch_multiple_hunks_succeeds` | false |
| `apply_patch_context_mismatch` | context 不一致 → `patch_conflict` | `apply_patch_rejects_context_mismatch` | false |
| `apply_patch_overlapping_hunks` | hunk 重複拒否 | `apply_patch_rejects_overlapping_hunks` | false |
| `apply_patch_rejects_headers` | `--- a/file` 等 header 付き拒否 | `apply_patch_rejects_diff_headers` | false |
| `apply_patch_empty_invalid` | 空 patch → `invalid_patch` | `apply_patch_rejects_empty_patch` | false |
| `apply_patch_no_change` | 同一結果 → 成功・書込なし | `apply_patch_no_change_skips_write` | false |
| `apply_patch_crlf` | CRLF ファイルで CRLF 維持 | `apply_patch_preserves_crlf` | false |
| `apply_patch_mixed_line_endings` | mixed 拒否 | `apply_patch_rejects_mixed_line_endings` | false |
| `apply_patch_size_limit` | patch 上限超過 | `apply_patch_enforces_patch_size_limit` | false |
| `race_stale_apply_patch` | 承認待ち中の外部変更 | `apply_patch_detects_stale_file_after_approval_wait` | false |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| adapter | `aibe/src/adapters/outbound/tools/apply_patch.rs` |
| adapter | `aibe/src/adapters/outbound/tools/patch_parser.rs`（必要なら） |
| adapter | `aibe/src/adapters/outbound/tools/file_change_common.rs`（拡張） |
| application | `aibe/src/application/tool_defs.rs` |
| tests | `aibe/tests/apply_patch.rs` |

## 4. 実装手順

### 4.1 引数（§10.2）

```json
{
  "path": "src/main.rs",
  "expected_sha256": "...",
  "patch": "@@ -10,3 +10,4 @@\n..."
}
```

path 引数のみ正本。patch 本文から path を解決しない。

### 4.2 parser 要件（§10.5）

- subprocess / shell 禁止
- fuzzy match 禁止
- whitespace 暗黙補正禁止
- ライブラリ使用時も上記を保証。不可なら subset parser を自前実装

### 4.3 `no_change`（§10.4）

結果が元と同一なら成功、`decision = no_change`、承認も書込もスキップ。

### 4.4 Phase 6 との接続

`file_change_common` 経由で `FileChangeService` へ `operation = patch` を渡す。journal / atomic / revalidate は共通化。

## 5. 本 Phase で返すエラー語彙

`invalid_patch`, `patch_conflict`, `unsupported_line_endings`, `input_too_large`, `stale_file`, `precondition_required`

## 6. 検証

```bash
./scripts/verify-targeted.sh --package aibe
cargo test -p aibe apply_patch -j 1
./scripts/verify.sh
```
