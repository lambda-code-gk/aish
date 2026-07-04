# 0055 — Collaborative Human Handoff Phase 1 実装指示書

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **マスター**: [0055_collaborative-human-handoff-implementation-spec.md](0055_collaborative-human-handoff-implementation-spec.md)  
> **状態**: Phase 1 完了 / Phase 2–5 未着手  
> **前提**: なし

## 0. 目的

Handoff / lease / checkpoint / command candidate の **domain 型と永続化契約** を追加する。PTY・CLI・agent loop 統合は **本 Phase では行わない**。

## 1. スコープ

### 1.1 対象

| 項目 | 設計参照 |
|------|----------|
| `HandoffState` 列挙（`CANCELLED` 含む）と遷移関数 | spec §9 |
| `Handoff` 全主要フィールド（§24.1） | spec §15, §24.1 |
| `HandoffLease` 全フィールド（§24.2） | spec §15.3 |
| `HandoffShellSession`, `CommandCandidate`（§21） | spec §8, §21 |
| `RequestedShellExec`, `HumanHandoffResult`（§26） | spec §15, §26 |
| `CollaborativeAgentRole`, `CollaborativePolicy` | spec §6 |
| `HandoffRepository`, `LeaseRepository`, `CheckpointRepository` port | spec §13 |
| `CancelHandoff` domain 操作（shell 起動前失敗用） | spec §16 |
| `FilesystemHandoffStore` adapter（`~/.local/share/aibe/handoffs/`） | spec §3.2 |
| token hash 保存（平文は store しない） | spec §13, §23 |
| `candidate_command` 組み立て（`command` + `args[]`、分解禁止） | spec §3.1, §8 |
| child goal メタ型（memory `goal` 連携用 ID のみ） | spec §10 |
| unit test（遷移、lease 排他、stale generation） | spec §28.1 |

### 1.2 非対象（後 Phase）

| 項目 | Phase |
|------|-------|
| `--collaborative` CLI | 2 |
| human shell 起動 | 2 |
| `ai status` / `resume` | 3–4 |
| side agent | 3 |
| wire protocol 本番経路への組み込み | 2–3 |

## 2. 受け入れ条件（`spec = "0055"`）

| id | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `handoff_state_transitions` | 正常・side・異常遷移が domain で検証される | `handoff_state_transitions_are_validated` | false |
| `lease_exclusive` | 同時 lease 取得の 2 件目が拒否される | `handoff_lease_rejects_concurrent_owner` | false |
| `token_hash_not_plaintext` | store が平文 token を保持しない | `handoff_store_persists_token_hash_only` | false |
| `shell_generation_monotonic` | 新 session で generation が増加し旧 token が失効扱い | `shell_session_generation_invalidates_old_token` | false |
| `candidate_command_no_split` | `command`+`args` から候補を組み立て、`\|\|` 等を分解しない | `candidate_command_preserves_shell_operators_in_args` | false |
| `candidate_source_preserved` | PARENT_AGENT 等 source が保持される | `command_candidate_source_roundtrip` | false |
| `child_goal_close_reason` | 終了時 `close_reason=control_returned` | `child_goal_records_control_returned_not_achievement` | false |
| `checkpoint_required_fields` | checkpoint に必須フィールドが含まれる | `checkpoint_contains_required_recovery_fields` | false |
| `human_handoff_result_dto` | `HumanHandoffResult` serde roundtrip | `human_handoff_result_serde_roundtrip` | false |

## 3. 変更ファイル（目安）

| 区分 | パス |
|------|------|
| protocol | `aibe-protocol/src/collaborative_handoff.rs`（新設）, `lib.rs` |
| domain | `ai/src/domain/collaborative_handoff.rs`（新設）, `mod.rs` |
| ports | `ai/src/ports/outbound/handoff_repository.rs` 等 |
| adapters | `ai/src/adapters/outbound/handoff_store.rs` |
| tests | `ai/tests/0055_collaborative_handoff_red.rs`, `aibe-protocol/tests/0055_collaborative_handoff_red.rs` |

## 4. 実装手順

### 4.1 protocol DTO

```rust
// execution_outcome, requested_command_completion, handoff_id 等
pub struct HumanHandoffResult { ... }
pub enum HandoffExecutionOutcome { HumanControlReturned, ... }
pub enum RequestedCommandCompletion { Unknown, ... }
```

### 4.2 domain 遷移

純関数 `fn try_transition(state: HandoffState, event: HandoffEvent) -> Result<HandoffState, HandoffTransitionError>`。

`ORPHANED` から `HUMAN_ACTIVE` は `Resume` のみ。`CREATING` + shell 起動失敗 → `CANCELLED`。

Checkpoint 必須フィールドは設計書 spec §24.3 と同一。

### 4.3 filesystem store

- `conversation_store.rs` と同様の lock + atomic rename
- `lease.json` 更新は write-to-temp + rename
- `events.jsonl` に token / command 全文を **無条件出力しない**（ID と state のみ）

### 4.4 candidate 組み立て

```rust
pub fn build_candidate_command(command: &str, args: &[String]) -> String
```

`args` 各要素は `shell_escape`（既存があれば再利用、無ければ最小実装）。`command` 文字列は変更しない。

## 5. 検証

```bash
./scripts/verify-targeted.sh --package ai
./scripts/verify-targeted.sh --package aibe-protocol
cargo test -p ai --test 0055_collaborative_handoff_red -j 1
```

Phase 完了時: 上記 AC を `pending = false`、`#[ignore]` 解除後 `./scripts/verify.sh`。

## 6. 完了後

Phase 2 指示書へ進む。human shell を起動する前に checkpoint 保存が完了していることを Phase 2 の統合テストで検証する。
