# 0054 — Safe File Write Tools Phase 5 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **マスター**: [0054_safe-file-write-tools-implementation-spec.md](0054_safe-file-write-tools-implementation-spec.md)  
> **状態**: 実装済み（Phase 5）  
> **前提**: [Phase 4](0054_safe-file-write-tools-phase4-implementation-spec.md) 完了

## 0. 目的

設計書 §17 の **prepare → approve → revalidate → journal → commit** を `FileChangeService` として実装し、§14.5–14.6 の **`ToolApprovalGate`** と §20 の **監査（sanitized arguments / approval_state）** を接続する。本 Phase では **fake gate** で統合テストし、実 UI は Phase 8。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `FileChangePlan` / `FileChangeService` | §17, §24.2 |
| `ToolApprovalGate` port | §14.5 |
| `ToolExecutionContext` への gate 注入 | §14.5 |
| connection-level approval adapter 整理 | §14.5 |
| 承認待ち timeout（既定 10 分、`exec_timeout_ms` 不含） | §14.6 |
| turn cancellation で承認待ち終了 | §14.6 |
| 承認後 revalidate（hash / 新規存在チェック） | §17.3 |
| `tool_disabled` vs `write_denied_by_policy` 分岐 | §13.1, §17.1 |
| sanitized `ExecutedToolCall.arguments` | §20.1 |
| `approval_state` / `approval_source` | §20.3 |
| `dry_run = false` 固定 | §20.4 |

### 1.2 非対象

| 項目 | Phase |
|------|-------|
| `write_file` / `apply_patch` tool executor | 6–7 |
| `ai` TTY 承認 UI | 8 |
| `@edit` | 6 |

## 2. 受け入れ条件

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `file_change_prepare_no_write` | prepare 段階でファイルを変更しない | `file_change_prepare_does_not_mutate_file` | false |
| `policy_never_denies` | `approval=never` → `write_denied_by_policy` | `file_write_never_mode_denies_execution` | false |
| `policy_always_skips_prompt` | `approval=always` は prompt なしで監査残し | `file_write_always_mode_skips_prompt` | false |
| `tool_disabled_when_config_off` | `enabled=false` → `tool_disabled` | `file_write_disabled_returns_tool_disabled` | false |
| `fake_gate_yes_commits` | fake gate yes で commit まで到達 | `file_change_fake_gate_yes_commits` | false |
| `fake_gate_no_denies` | fake gate no で変更なし | `file_change_fake_gate_no_leaves_file_unchanged` | false |
| `revalidate_stale_file` | 承認待ち中の外部変更 → `stale_file` | `file_change_revalidate_detects_stale_file` | false |
| `cancel_during_approval` | cancel で書き込まない | `file_change_cancel_during_approval_writes_nothing` | false |
| `approval_gate_missing` | gate 不在 → `approval_unavailable` | `file_change_missing_gate_returns_unavailable` | false |
| `sanitized_arguments` | raw content/patch が監査に入らない | `file_change_sanitizes_executed_tool_arguments` | false |
| `tool_approval_wire_roundtrip` | socket 上で prompt/approval roundtrip | `tool_approval_wire_roundtrip` | false |

## 3. エラー分岐表（必須）

| 条件 | エラー |
|------|--------|
| `[tools.file_write].enabled = false` | `tool_disabled` |
| `approval = never` | `write_denied_by_policy` |
| `approval = ask` かつ gate 不在 | `approval_unavailable` |
| 承認拒否 | `approval_denied` |
| revalidate で hash 不一致 / 新規が既存化 | `stale_file` |
| journal 保存失敗 | `journal_failed` |

## 4. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| application | `aibe/src/application/file_change_service.rs` |
| domain | `aibe/src/domain/file_change.rs` |
| ports | `aibe/src/ports/outbound/tool_approval.rs` |
| adapter | `aibe/src/adapters/inbound/connection_approval.rs` |
| application | `aibe/src/application/tool_round/executor.rs`（cancellation 連携） |
| tests | `aibe/tests/file_change_service.rs` |
| tests | `aibe/tests/tool_approval_socket.rs` |

## 5. 実装手順

### 5.1 フロー順序（§17 — 厳守）

1. parse / enabled / capability / path / read / validate / plan / diff preview
2. **（まだ書かない）**
3. approval（never / ask+gate / always）
4. revalidate
5. journal
6. atomic commit

### 5.2 fake gate テスト

`TestToolApprovalGate` を test-only で用意し、yes/no/cancel/遅延を制御。Phase 8 まで本番 UI に依存しない。

### 5.3 監査（§20）

`write_file` sanitized: `path`, `mode`, `expected_sha256`, `content_bytes`  
`apply_patch` sanitized: `path`, `expected_sha256`, `patch_bytes`, `hunk_count`

## 6. 検証

```bash
./scripts/verify-targeted.sh --package aibe
cargo test -p aibe file_change_service -j 1
cargo test -p aibe tool_approval -j 1
./scripts/verify.sh
```
