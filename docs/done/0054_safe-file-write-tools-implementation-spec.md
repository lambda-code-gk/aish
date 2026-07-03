# 0054 — Safe File Write Tools 実装指示書（マスター）

> **種別**: 実装指示書（`docs/tasks/`）  
> **設計正本**: [0054_safe-file-write-tools-spec.md](../spec/0054_safe-file-write-tools-spec.md)  
> **状態**: 進行中  
> **起票**: 2026-07-02

## 0. 目的

AIBE に `write_file` / `apply_patch` を first-class tool として追加し、安全なテキストファイル作成・変更を実現する。設計書 §0–§30 の正本は `docs/spec/0054_safe-file-write-tools-spec.md` とする。

本マスターは **Phase 分割と実行順** の索引である。実装は **Phase 1 から順に**、各 Phase の個別指示書 1 本ずつ実行する。

## 1. Phase 一覧と実行順

| Phase | 指示書 | 内容 | ゲート |
|-------|--------|------|--------|
| 1 | [phase1](0054_safe-file-write-tools-phase1-implementation-spec.md) | protocol / tool 名 / config / capability / 承認 DTO / registry | 当該 Phase の `spec-acceptance.toml` がすべて `pending = false` |
| 2 | [phase2](0054_safe-file-write-tools-phase2-implementation-spec.md) | safe_path / SHA-256 / text 検証 / read_file 移行 | 同上 |
| 3 | [phase3](0054_safe-file-write-tools-phase3-implementation-spec.md) | `read_file` metadata（`include_metadata`） | 同上 |
| 4 | [phase4](0054_safe-file-write-tools-phase4-implementation-spec.md) | diff / atomic write / journal | 同上 |
| 5 | [phase5](0054_safe-file-write-tools-phase5-implementation-spec.md) | FileChangeService / ToolApprovalGate / 監査 | 同上 |
| 6 | [phase6](0054_safe-file-write-tools-phase6-implementation-spec.md) | `@edit` / `write_file` / `file_change_common` | 同上 |
| 7 | [phase7](0054_safe-file-write-tools-phase7-implementation-spec.md) | `apply_patch` strict parser | 同上 |
| 8 | [phase8](0054_safe-file-write-tools-phase8-implementation-spec.md) | `ai` 承認 UI / `aibe-client` callback | 同上 |
| 9 | [phase9](0054_safe-file-write-tools-phase9-implementation-spec.md) | 統合テスト / manual / docs 同期 | 同上 |

**禁止**: 前 Phase の `pending = true` のまま次 Phase に進む。

## 2. 実行方法（1 Phase ずつ）

各 Phase 指示書を Cursor / Codex に渡すときは、次の文面を使う。

```text
docs/tasks/0054_safe-file-write-tools-phaseN-implementation-spec.md を実装してください。
設計正本: docs/spec/0054_safe-file-write-tools-spec.md
前提: Phase 1..N-1 完了（spec-acceptance.toml の当該 Phase より前が pending = false）
完了時: 当該 Phase の spec-acceptance が pending = false、./scripts/verify.sh 成功
```

## 3. 受け入れ条件レジストリ

正本: `scripts/spec-acceptance.toml`（`spec = "0054"`）。各 Phase 指示書に当該 Phase の AC 一覧がある。

未到達 AC は **`#[ignore]` 付きテストを先に追加** し、`pending = true` で登録する。Phase 完了時に `pending = false` と `#[ignore]` 解除。

RED スタブ（全 Phase 共通・実装前に配置済み）:

| クレート | ファイル |
|----------|----------|
| aibe-protocol | `aibe-protocol/tests/0054_safe_file_write_red.rs` |
| aibe | `aibe/tests/0054_safe_file_write_red.rs` |
| ai | `ai/tests/0054_safe_file_write_red.rs` |
| aibe-client | `aibe-client/tests/0054_safe_file_write_red.rs` |

Phase 完了時は該当テストを実装ファイルへ移し、RED から削除する。

## 4. 全体完了条件（§30）

1. Phase 1–9 の `spec-acceptance.toml` がすべて `pending = false`
2. `./scripts/verify.sh` 成功
3. `docs/architecture.md` / `docs/security.md` / `docs/testing.md` / `docs/manual/` 同期（Phase 9）
4. 本マスターと各 Phase 指示書を `docs/done/` へ移動し、`docs/0000_spec-index.md` を「実装済み」に更新

## 5. 意図的な Phase 分割判断

| 判断 | 理由 |
|------|------|
| `@edit` は Phase 6 まで追加しない | Phase 1 で公開すると未実装 tool が露出する（設計 §6） |
| 承認 UI は Phase 8 | Phase 5–7 は fake `ToolApprovalGate` でサーバ E2E を先に固める |
| `read_file` path 移行は Phase 2 | 設計 §12.2。write 専用コピーを禁止 |
| `file_change_common` は Phase 6 冒頭 | `write_file` / `apply_patch` の重複実装を防ぐ |

## 6. 仕様との差分

- なし（設計書どおり）
