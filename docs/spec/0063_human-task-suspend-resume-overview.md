# 0063 Collaborative Mode Human Task 中断・再開 — 親概要

> **種別**: 0063 系列の親概要（実装 Scope Lock ではない）  
> **状態**: 設計確定  
> **正本**: GitHub Issue #6 のローカルコピー [`issue-6-human-task-suspend-resume.md`](../todo/issue-6-human-task-suspend-resume.md)  
> **最初の実装単位**: [`0063_human-task-suspend-checkpoint-spec.md`](0063_human-task-suspend-checkpoint-spec.md)

## 1. 目的と不変条件

明示 `human_task` から開始した Human Task を意図的に中断し、`ai` コマンド終了後に新しい Human Shell で再開できるようにする。再開後に作業が完了したら、保存した結果から新しい Collaborative Mode ターンを開始する。

再開する正本は `HumanTaskRequest`、briefing、作業 cwd、segment ごとの Evidence / Observation、および新しい継続ターンに必要な最小の親コンテキストである。同一の LLM request / provider stream / Unix socket / aibe agent loop / ai・aish process / PTY は復元しない。0055 の旧 `shell_exec` handoff は対象外である。

## 2. Scope Gate と分割

Issue 全体を一つの feature として数えると、Human Task workflow、永続 aggregate、Human Shell suspend protocol、aibe turn停止、resume、segment/Evidence統合、agent continuation、recovery ownership が同居する。novel mechanism が複数となり、crash recovery / schema migration / lease のいずれかも含めれば Complexity Gate は **Red** である。feature class に承認例外はないため、次の独立 Vertical Slice に分割する。

| 論理区分 | 成果 | 設計上の扱い |
|----------|------|--------------|
| 0063-A+B | 明示 Human Task を中断・安全に保存し、aibe turnを追加 LLM 呼び出しなしで終了して `status` で確認し、local `cancel` で復旧できる | 最初の正式 spec 0063。Aだけでは本番作成経路がなく Vertical Slice にならないため最小Bを同梱 |
| 0063-C | `ai human-task resume`、新segment、複数回中断、全segment Evidence統合 | 0063 完了後の別番号 spec。resume cwd 不在時は Suspended を維持 |
| 0063-D | 保存した親コンテキストと最終結果から新しい Collaborative Mode 継続ターンを開始 | C完了後の別番号 spec。`ResultPending` と同一process内重複防止を扱う |
| 0063-E | stale ownership、予期しないprocess終了、破損checkpointの追加回復UX、権限・session pruning競合の hardening | MVP外の別番号 spec。0063本体のoperation-scoped root flock、local cancel、fail-closedな破損拒否・owner/mode/symlink検査は後送せず、crash recovery / lease / migration が必要なら STOP-THE-LINE |

後続 spec の4桁番号は各設計開始時の最新 index に従って採番する。論理名 `0063-C`〜`0063-E` は Issue との対応を示すラベルであり、複数の正式 spec が同じ `feature-scope.toml` key を共有することはしない。

## 3. 段階間の契約

- Checkpoint の唯一の所有者は `ai`。`aish` は一つの Human Shell segment と control event だけを担当し、永続checkpointを読まない。
- `aibe` は `Suspended` を非エラーの turn終端として扱い、同じ tool roundの後に LLM を呼ばない。
- 最初の spec で将来必要な version 1 envelope と親コンテキストを保存し、後続で旧schema migrationを要求しない。
- `Suspended` は最終作業結果ではない。`Done` / `Blocked` / `Cancelled` と区別する。
- raw shell log はcheckpointへ複製しない。各segment終了時に bounded Evidence / Observation を取り込む。
- 通常の Ctrl+D / `exit` は従来どおり同期 `Done` で同じ親agentへ戻す。中断しない場合にdurable checkpointを残さない。

## 4. Fault model

系列のMVPは単一ホスト・単一ユーザーを前提とする。意図的な `human-task suspend` と正常な `ai` 終了をまたぐ保存、およびcreate/status/cancelのroot flockによるprocess間直列化は製品機能として保証するが、process crash後の所有権復旧、OS再起動中の書込み、exactly-once、旧schema migrationは保証しない。

`Running` checkpoint の所有process消失を検出して自動 `Suspended` にする処理、PID/lockからの所有権復旧、lease / heartbeat / reconciler は 0063-E 候補であり、前段へ持ち込まない。

## 5. パック構成の適用

**No** — 0045 §6 の候補条件は満たさない。中断・再開は明示 `human_task` の lifecycle 契約そのもので、無効時Basic runtimeや重い依存の脱着を提供するoptional機能ではない。`aish` はそもそもパック構成対象外であり、`ai` / `aibe` の横断点も既存 port、protocol、agent turn policyを必要最小限に拡張する。Pack境界、Active/Basic Pack、runtime toggle、Cargo featureは導入しない。
