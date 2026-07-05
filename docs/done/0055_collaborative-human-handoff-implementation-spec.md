# 0055 — Collaborative Human Handoff 実装指示書（マスター）

> **種別**: 実装指示書（`docs/done/`）  
> **設計正本**: [0055_collaborative-human-handoff-spec.md](../spec/0055_collaborative-human-handoff-spec.md)  
> **状態**: 実装済み  
> **起票**: 2026-07-04

## 0. 目的

Human-in-the-loop 協調作業（親 `shell_exec` → human shell → 再観測）を **Phase 1 から順に** 実装する。本マスターは Phase 分割と実行順の索引である。

## 1. Phase 一覧

| Phase | 指示書 | 内容 | 主なクレート |
|-------|--------|------|-------------|
| 1 | [phase1](0055_collaborative-human-handoff-phase1-implementation-spec.md) | Domain、永続化、lease、checkpoint、candidate、unit test | `ai`, `aibe-protocol` |
| 2 | [phase2](0055_collaborative-human-handoff-phase2-implementation-spec.md) | `--collaborative`、親 handoff、human shell、recall、親再観測 | `ai`, `aish`, `aibe-client` |
| 3 | [phase3](0055_collaborative-human-handoff-phase3-implementation-spec.md) | side agent、token 検証、`--standalone`、`ai status`、人間待ち | `ai`, `aibe` |
| 4 | [phase4](0055_collaborative-human-handoff-phase4-implementation-spec.md) | ORPHANED、`ai resume`、heartbeat、fault injection | `ai`, `aish` |
| 5 | [phase5](0055_collaborative-human-handoff-phase5-implementation-spec.md) | prompt UX、signal、redaction、docs / manual | `aish`, `ai`, `docs/` |
| 6 | [phase6](0055_collaborative-human-handoff-phase6-implementation-spec.md) | Durable workflow 再設計、縦切り E2E、failure injection | `ai`, `aish`, `scripts/` |

**禁止**: 前 Phase の `pending = true` のまま次 Phase に進む。

## 2. 実行方法（1 Phase ずつ）

```text
docs/tasks/0055_collaborative-human-handoff-phaseN-implementation-spec.md を実装してください。
設計正本: docs/spec/0055_collaborative-human-handoff-spec.md
前提: Phase 1..N-1 完了（spec-acceptance.toml の当該 Phase より前が pending = false）
完了時: 当該 Phase の spec-acceptance が pending = false、./scripts/verify.sh 成功
```

## 3. 受け入れ条件レジストリ

正本: `scripts/spec-acceptance.toml`（`spec = "0055"`）。

未到達 AC は **`#[ignore]` 付きテストを先に追加** し、`pending = true` で登録。Phase 完了時に `pending = false` と `#[ignore]` 解除。

RED スタブ（全 Phase 共通・実装前に配置）:

| クレート | ファイル |
|----------|----------|
| ai | `ai/tests/0055_collaborative_handoff_red.rs` |
| aibe-protocol | `aibe-protocol/tests/0055_collaborative_handoff_red.rs` |
| aish | `aish/tests/0055_collaborative_handoff_red.rs` |

Phase 完了時は該当テストを実装モジュールへ移し、RED から削除する。

## 4. 全体完了条件

1. Phase 1–6 の `spec-acceptance.toml` がすべて `pending = false`
2. `./scripts/verify.sh` 成功
3. `docs/architecture.md` / `docs/security.md` / `docs/manual/` 同期（Phase 5）
4. 本マスターと各 Phase 指示書を `docs/done/` へ移動し、`docs/0000_spec-index.md` を更新

## 5. クレート境界（厳守）

| クレート | 協調作業での責務 |
|----------|------------------|
| `ai` | CLI、handoff application、side agent 起動、status/resume |
| `aibe` | tool result 拡張、side agent turn（既存 loop） |
| `aish` | human shell PTY、prompt hook、control channel |
| `aibe-protocol` | 共有 DTO（`HumanHandoffResult` 等） |

`aish` は LLM / aibe socket を持たない。`ai` は LLM 直叩き禁止（既存境界）。

## 6. 意図的な Phase 分割判断

| 判断 | 理由 |
|------|------|
| human shell は Phase 2 | Phase 1 で永続化・遷移を unit test 固定してから統合 |
| handoff env **設定**は Phase 2、**検証・接続**は Phase 3 | spec §31 |
| side agent は Phase 3 | 親 handoff 正常経路を先に E2E 化 |
| 会話要約（§25）は Phase 3 | side turn と同時に必要 |
| 復旧は Phase 4 | 正常フローとテスト基盤が無いと fault injection が不安定 |
| UX 仕上げは Phase 5 | 機能正本は Phase 1–4 で満たす |
| `shell_exec` インターセプト | `CollaborativeShellExecPolicy`（承認 UI 置換ではない）— spec §30 |

## 7. 受け入れ条件と設計書 §29 の対応

正本: 設計書 spec §32 の表。**件数と Phase 割当の正本は `scripts/spec-acceptance.toml` の `spec = "0055"` エントリ**（各 Phase 指示書 §2 と 1:1 で同期する）。

Phase 完了の定義: 当該 Phase の全 AC が `pending = false` かつ `#[ignore]` 解除済み。
