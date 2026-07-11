# 0058 AISH Doctor / 常用診断導線 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **起票**: 2026-07-11  
> **関連**: [`docs/feature-development-policy.md`](../feature-development-policy.md)、[`0027_ai-ux-spec.md`](0027_ai-ux-spec.md)、[`0045_pack-composition-spec.md`](0045_pack-composition-spec.md)、[`0049_aish-command-output-replay-spec.md`](0049_aish-command-output-replay-spec.md)、[`0050_client-provided-replay-tool-spec.md`](0050_client-provided-replay-tool-spec.md)、[`0051_smart-observation-report-spec.md`](0051_smart-observation-report-spec.md)、[`0054_safe-file-write-tools-spec.md`](0054_safe-file-write-tools-spec.md)、[`0055_minimal-human-handoff-spec.md`](0055_minimal-human-handoff-spec.md)

## 0. Core outcome

ユーザーが `ai doctor` を実行すると、日常利用の前提が OK/WARN/FAIL の段階的チェックとして可視化され、次に直すべき一手が提示される。

## 1. Minimum vertical slice

```text
ai doctor
→ 既存の local config / session / shell-log 解決と aibe Ping/Pong を副作用なしで実行
→ HealthCheck の checks[] へ正規化
→ human 要約（`--format` 省略時）または明示した json / tsv / env
→ FAIL の有無に応じた exit code
```

本 spec は構想上の Slice 1（構造化）と Slice 2（日常 preflight）の最小垂直スライスだけを扱う。実装 Phase は一つとし、`doctor_health_checks_render_human_and_json` を縦断 AC とする。

## 2. Fault model

### 2.1 保証対象

標準 Fault Model に従う。診断は単一ホスト・単一ユーザーで実行時点のスナップショットを返す。各 check は可能な限り独立に評価し、一項目の FAIL で残りの local check を打ち切らない。ただし設定を安全に読めず後続判定の前提がない場合、その依存 check は WARN または FAIL と理由を明示する。

通常の `ai doctor` は daemon 起動、外部 LLM API、provider 接続、ファイル更新、process restart を行わない。socket check は既存の timeout 付き Ping/Pong を使い、`ensure_running` は呼ばない。

### 2.2 保証対象外

- 実行後に変化した socket、session、log の継続的監視
- OS / process crash 後の診断再開
- 複数ホスト、複数ユーザー、exactly-once な診断
- provider やインターネットの可用性
- Ping/Pong より詳細な client/server protocol version negotiation（現行 wire DTO に version 情報はない）

## 3. Non-goals

- Slice 3 の `--full` / `--network` / provider connectivity / LLM dry-run
- `--fix-plan`、自動 `--fix`、daemon restart、config 書き換え
- 独立した `aish doctor` CLI
- handoff 用の実 shell 起動、smart 自動チューニング、MCP 診断
- PATH、重複 install、全バイナリ version の網羅診断
- 0049/0050 replay の可用性・内容読出し、0051 observation の統計評価、0054 safe-write の書込み試行、0055 handoff の起動
- 新しい protocol DTO または aibe RPC

## 4. Complexity inventory

| 項目 | 数・内容 |
|------|----------|
| 実行主体 | 1（`ai`） |
| 状態機械 | 0 |
| 永続 aggregate | 0 |
| 外部副作用 | 1（既存 Unix socket への timeout 付き Ping/Pong。read-only） |
| プロセス境界 | 1（`ai` → `aibe` socket） |
| 新規基盤機構 | 1（`health-check-report`） |
| 他機能統合 | 1（`ai` の既存 diagnostics 解決経路） |

`scripts/feature-scope.toml` の `0058` entry と一致させる。replay / smart / safe-write / handoff の診断は行わず、二つ目の feature integration を持ち込まない。

## 5. Complexity Gate

- 判定: **Green**
- 理由: 実行主体 1、状態機械・永続 aggregate 0、read-only effect と process boundary が各 1、新規機構も HealthCheck 表現の一つだけで Green 上限内である
- 分割判断: network/provider、自動修復、独立 `aish` CLI、replay を含む他機能診断を Deferred へ送り、既存 `ai` diagnostics の構造化に限定する
- 承認例外: なし

One Novelty Rule 上の novelty は `health-check-report` 一つだけである。

## 6. Complexity budget

| 項目 | 上限 |
|------|------|
| 新規実行主体 | +0 |
| 状態機械 / 永続 aggregate | +0 / +0 |
| 新規 protocol DTO / RPC | +0 / +0 |
| 外部 API / process 起動 /書込み副作用 | +0 |
| 新規診断 check | 下表の 6 件まで |
| 新規出力形式 | +1（`doctor` の `--format` 省略時だけ human。明示 `json / tsv / env` は既存形式） |

## 7. Split triggers

次が必要になったら STOP-THE-LINE し、0058 へ追加せず別 spec に分割する。

- provider / LLM / internet への接続
- `aibe` の起動・再起動、外部 command または実 shell の起動
- config、session、log その他のファイル更新
- protocol version を返す新規 DTO / RPC
- background watcher、daemon、lease / heartbeat / reconciler
- PATH / install provenance の横断探索
- MCP server や smart observation 内容の健全性評価

## 8. パック構成の適用

**No** — 0045 §6 の適用候補（optional runtime、core service への横断 hook、重い依存のリンク除外、専用 RPC 群）に該当しない。`ai doctor` は basic runtime 自体の常用診断であり、無効化可能な機能束ではなく core CLI に固定する。診断対象の optional 機能が無効な場合は HealthCheck の WARN/OK として表現し、Active/Basic Pack や構成ルートを追加しない。

## 9. Acceptance Criteria

### 9.1 モデル

```text
CheckStatus = OK | WARN | FAIL
HealthCheck = { id, status, message, suggestion }
DoctorReport = { command, status, checks[] }
```

`id` は安定した machine-readable lower snake case、`status` は JSON で `ok|warn|fail` とする。`message` は観測事実を秘密情報なしで短く説明する。`suggestion` は OK では `null` を許し、WARN/FAIL ではユーザーが次に実行・確認する一手を必須とする。report の総合 status は `FAIL > WARN > OK` の最大深刻度で決める。checks は次表の順序で固定する。

### 9.2 本 spec の check 一覧

| check ID | 判定範囲 |
|----------|----------|
| `socket_reachable` | 設定または `--socket` で解決した Unix socket へ接続し request を送受信できるか。未到達または timeout は FAIL とする。socket へ接続できたが応答を Pong として decode できない場合は、`socket_reachable` を OK、`protocol_compatibility` を FAIL とする |
| `session_context` | `AISH_SESSION_DIR` と implicit session id を既存規則で解決できるか。通常利用に必須でない未設定は WARN |
| `shell_log_readable` | 既存 `resolve_shell_log_info` が選んだ log path の存在・read 可否。明示要求された log の解決失敗は FAIL、利用しない設定は WARN |
| `tools_configuration` | 既存 tool token / group 解決が成功し、有効 tool 集合を説明できるか。不正 token は FAIL、空集合は WARN |
| `output_filter_configuration` | filter の有無・source・masked 状態を既存秘匿規則で確認する。設定ファイル全体の読込または parse に失敗した場合も FAIL とし、message は filter 単体ではなく config 読込/parse 失敗であることが分かる表現にする。設定済みで内容を露出せず解決できれば OK |
| `protocol_compatibility` | 現行経路では Ping request に Pong response を decode できることを wire 互換の根拠とする。成立時 OK、不正応答は FAIL。client/server の数値 version 表示は Deferred |

### 9.3 出力・exit code・`ai status` 互換

- `ai doctor` だけは `--format` 省略時に stdout へ総合 `OK` / `WARN` / `FAIL`、各 check の status と message、WARN/FAIL の suggestion を human-readable に出す。共有 `OutputFormatArg` の既定値変更で `status` 等へ波及させず、doctor の「format 指定なし」を CLI 層で区別する
- `ai doctor --format json` は `DoctorReport` を一つ出し、最低限 `command`, `status`, `checks[]` と各 check の `id/status/message/suggestion` を持つ。診断以外の文を stdout に混ぜない
- `--quiet` は stderr の補助説明だけを抑止し、stdout の診断 report と exit code は変えない
- exit code は FAIL なし（OK/WARN）を `0`、一つ以上 FAIL を `1`、CLI usage / report 構築自体のエラーを既存どおり `2` 以上とする
- `ai status` は 0027 の canonical local status として既存 `DiagnosticsReport`、`--format json|tsv|env`、socket 未到達でも成功する挙動を維持する。0058 では deprecate しない
- `ai doctor --format tsv|env` の既存受理も互換のため維持し、従来の `DiagnosticsReport` key を削除・改名せず、HealthCheck を安定 key（`check.<id>.status/message/suggestion`）として追記する。`--format json` は 0058 の `DoctorReport` schema へ移行する破壊的変更であり、現行 alias の JSON schema 互換は保証しない

判定テストは環境・設定・socket 応答・一時ファイルを fixture として制御する。`socket_reachable` と `protocol_compatibility` は一回の Ping 観測を共有し、接続不能なら両方 FAIL、接続後の不正応答なら `socket_reachable` は OK かつ `protocol_compatibility` は FAIL とする。これにより外部 API や実 daemon に依存せず全分岐を再現できる。

### 9.4 AC 表

| ID | 条件 |
|----|------|
| `doctor_alias_uses_doctor_command_name` | 現行互換の設計基点として、`ai doctor --format json` が doctor command として dispatch される |
| `doctor_health_checks_render_human_and_json` | `ai doctor` の一回の診断が同じ順序・判定の checks を human と JSON の双方で表現する |
| `doctor_json_has_stable_check_schema` | JSON が `command/status/checks[]` と `id/status/message/suggestion` の安定 schema を持つ |
| `doctor_preflight_covers_locked_checks` | 本 spec の 6 check ID が表の順に欠落なく一度ずつ出力される |
| `doctor_fail_exit_and_warn_success` | 一つ以上 FAIL なら exit 1、OK/WARN のみなら exit 0 になる |
| `doctor_does_not_call_external_llm_or_mutate` | 通常 doctor は LLM/provider API、daemon 起動、process 起動、設定・log 書込みを行わない |
| `doctor_masks_filter_and_secret_values` | filter command、API key、credential、log 本文を message/suggestion/JSON に出さない |
| `doctor_checks_continue_after_failure` | socket 等の一 check が FAIL しても独立 local checks を評価して report を返す |
| `doctor_protocol_check_uses_existing_ping_contract` | protocol check は新規 RPC なしで既存 Ping/Pong decode 成立だけを判定する |
| `status_legacy_output_remains_compatible` | `ai status` の既存 structured fields、形式、exit 方針を維持する |
| `doctor_tsv_env_remain_machine_readable` | 既存 tsv/env 指定を拒否せず、従来 key を維持した上で安定した check key を出力する |

設計段階では、Feature Scope checker が要求する vertical-slice anchor として既存互換 AC `doctor_alias_uses_doctor_command_name` だけを `scripts/spec-acceptance.toml` に登録する。新規 HealthCheck AC は実装指示書作成時に全 row を 1:1 登録し、`vertical_slice_ac_id` を `doctor_health_checks_render_human_and_json` へ切り替え、`scripts/feature-scope.toml` を `locked` に変更して Scope Lock を開始する。

## 10. Deferred specs

| 別 spec 候補 | Deferred 内容 |
|---------------|---------------|
| Doctor network/full diagnostics | Slice 3: `--full` / `--network`、provider connectivity、LLM dry-run、数値 protocol version negotiation |
| Doctor remediation planning | `--fix-plan` と、さらに後続の確認付き `--fix`。自動修復、daemon restart、config 書換え |
| `aish doctor` CLI | `aish` 単体の install / PATH / shell hook / log recorder 診断 |
| Replay availability diagnostics | 0049/0050 の mode / manifest / tail artifact を横断した可用性診断 |
| Runtime integration diagnostics | handoff 実 shell 起動、smart 自動チューニング、MCP 診断 |
| Installation provenance diagnostics | PATH、複数 install、binary/client/server version の網羅診断 |

## 11. Scope change log

| Revision | 分類 | 変更 | 理由 |
|----------|------|------|------|
| 1 | INITIAL | Slice 1 + Slice 2 の read-only `ai doctor` HealthCheck に限定 | 8 診断レイヤー + 3 Slice を一括実装せず、One Novelty Rule と Green gate を守るため |
| 2 | `BLOCKER_ORIGINAL_AC` | replay 診断を Deferred へ分離し 6 checks に固定。human 既定と明示 format、Ping 判定、doctor tsv/env 互換境界を明文化 | integration 数と Green gate を一致させ、AC を決定的にテスト可能にするため |
