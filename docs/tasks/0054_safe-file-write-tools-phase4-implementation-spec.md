# 0054 — Safe File Write Tools Phase 4 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 実装済み（Phase 4）  
> **前提**: [Phase 2](0054_safe-file-write-tools-phase2-implementation-spec.md) 完了（Phase 3 と並行可だが、本 Phase 開始前に Phase 2 必須）

## 0. 目的

設計書 §16、§18、§19 の **unified diff 生成・atomic write・rollback journal** を tool 非依存の adapter として実装する。承認フロー・tool executor は Phase 5 以降。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| unified diff 生成（新規 / 既存） | §16.1 |
| summary（operation, +/-lines, bytes） | §16.2 |
| `max_preview_bytes` による preview truncate | §16.3 |
| 同 dir への temp + rename atomic write | §18 |
| journal 保存（`~/.local/share/aibe/file-changes/`） | §19 |
| metadata.json / before.bin | §19.2–19.3 |
| retention / 容量上限 | §19.4 |
| `FileSnapshot` domain 型 | §24.2 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| `ToolApprovalGate` | 5 |
| `FileChangeService` オーケストレーション | 5 |
| tool result / `change_id` 返却 | 6–7 |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `unified_diff_new_file` | 新規ファイル diff が `/dev/null` 形式 | `unified_diff_formats_new_file` | false |
| `unified_diff_existing_file` | 既存ファイル diff が `--- a/` 形式 | `unified_diff_formats_existing_file` | false |
| `preview_truncation` | `max_preview_bytes` 超過で truncate + flag | `diff_preview_truncates_at_max_bytes` | false |
| `atomic_write_preserves_original` | temp/rename 失敗で元ファイル残存 | `atomic_write_preserves_original_on_failure` | false |
| `atomic_write_no_temp_leftover` | 成功後 temp が残らない | `atomic_write_removes_temp_file_on_success` | false |
| `journal_saves_before_bytes` | commit 前に before.bin が正確 | `journal_saves_before_state_bytes` | false |
| `journal_create_absent` | 新規作成は `before_state=absent`、before.bin なし | `journal_records_absent_before_for_create` | false |
| `journal_permissions` | dir `0700`、ファイル `0600` | `journal_uses_restricted_permissions` | false |
| `journal_retention_cleanup` | 期限切れ journal の best-effort 削除 | `journal_retention_cleanup_removes_expired` | false |
| `journal_capacity_exceeded` | 容量確保不可で write 拒否 | `journal_capacity_exceeded_blocks_write` | false |
| `journal_no_raw_patch` | metadata に raw patch を保存しない | `journal_metadata_excludes_raw_patch` | false |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| domain | `aibe/src/domain/file_change.rs` |
| ports | `aibe/src/ports/outbound/file_change_journal.rs`, `file_change_store.rs` |
| adapter | `aibe/src/adapters/outbound/file_change_journal.rs` |
| adapter | `aibe/src/adapters/outbound/tools/file_atomic.rs` 等 |
| adapter | `aibe/src/adapters/outbound/tools/diff_preview.rs` 等 |
| tests | `aibe/tests/file_change_journal.rs`, `aibe/tests/atomic_write.rs` |

## 4. 実装手順

### 4.1 diff（§16）

- 新規: `--- /dev/null` / `+++ b/<path>`
- 既存: `--- a/<path>` / `+++ b/<path>`
- preview truncate しても **実際の変更 bytes は truncate しない**

### 4.2 atomic write（§18）

禁止: `tokio::fs::write(target, content)` 直接。

手順: 同 dir に `.aibe-write-<random>.tmp` → 全書き込み → flush/sync → permission → rename → parent sync (best effort)。

### 4.3 journal（§19）

- 保存失敗時は **書き込まない**（`journal_failed`）
- LLM prompt / raw patch を保存しない
- `before.bin` だけで復元可能

## 5. 本 Phase で返すエラー語彙

`journal_failed`, `journal_capacity_exceeded`, `write_failed`

## 6. 検証

```bash
./scripts/verify-targeted.sh --package aibe
cargo test -p aibe file_change -j 1
cargo test -p aibe atomic_write -j 1
./scripts/verify.sh
```
