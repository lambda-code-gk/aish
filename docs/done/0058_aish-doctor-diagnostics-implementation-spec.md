# 0058 AISH Doctor / 常用診断導線 実装指示書

設計書: [`docs/spec/0058_aish-doctor-diagnostics-spec.md`](../spec/0058_aish-doctor-diagnostics-spec.md)

## 0. 目的

`ai doctor` を既存 `status` alias から、日常利用の前提を 6 個の `HealthCheck` として副作用なしで診断する常用 preflight へ変更する。`--format` 省略時は human、明示時は JSON / TSV / env を出し、FAIL の有無を exit code に反映する。一方、`ai status` の既存 `DiagnosticsReport`、structured output、socket 未到達時も成功する契約は維持する。正本は設計書であり、Deferred 項目を本実装へ持ち込まない。

## 0.1 Scope Lock

- Feature scope registry: `scripts/feature-scope.toml`
- Scope revision: `3`
- Complexity class: Green
- Vertical slice AC ID: `doctor_health_checks_render_human_and_json`
- Locked AC IDs:
  - `doctor_alias_uses_doctor_command_name`
  - `doctor_health_checks_render_human_and_json`
  - `doctor_json_has_stable_check_schema`
  - `doctor_preflight_covers_locked_checks`
  - `doctor_fail_exit_and_warn_success`
  - `doctor_does_not_call_external_llm_or_mutate`
  - `doctor_masks_filter_and_secret_values`
  - `doctor_checks_continue_after_failure`
  - `doctor_protocol_check_uses_existing_ping_contract`
  - `status_legacy_output_remains_compatible`
  - `doctor_tsv_env_remain_machine_readable`

Scope Lock 後に AC、check、外部 effect、integration を追加しない。必要になった場合は実装を止め、設計書 §7 と feature scope policy に従って scope revision と Complexity Gate を再審査する。

## 1. Phase 分割

設計どおり Phase は一つだけとする。

| Phase | 内容 | ゲート（`scripts/spec-acceptance.toml`） |
|-------|------|------------------------------------------|
| 1 | `ai doctor` の CLI 分岐、6 checks の構築、human / JSON / TSV / env 表示、exit code、`status` regression を一つの vertical slice として実装する | 0058 の全 AC。縦断ゲートは `doctor_health_checks_render_human_and_json` |

実装順序は、登録済み `#[ignore]` skeleton を fixture 付き統合テストへ置換し、ドメインモデルと診断構築、CLI dispatch、render の順とする。全 AC が通った時だけ `pending=false` と `#[ignore]` 解除を同時に行う。単一 Phase のため部分完了を 0058 完了として報告しない。

## 2. 受け入れ条件

| ID | 条件 | テスト関数 | pending |
|----|------|------------|---------|
| `doctor_alias_uses_doctor_command_name` | `ai doctor --format json` が doctor として dispatch される既存設計基点 | 同名、`ai/tests/phase_a_cli.rs` | false（維持） |
| `doctor_health_checks_render_human_and_json` | 同じ fixture の診断が同順序・同判定で human / JSON に出る | 同名、`ai/tests/0058_aish_doctor_diagnostics.rs` | true |
| `doctor_json_has_stable_check_schema` | `command/status/checks[]` と check の全 field を保証する | 同名、同上 | true |
| `doctor_preflight_covers_locked_checks` | 6 ID を固定順で一度ずつ出す | 同名、同上 | true |
| `doctor_fail_exit_and_warn_success` | FAIL は exit 1、OK/WARN のみは exit 0 | 同名、同上 | true |
| `doctor_does_not_call_external_llm_or_mutate` | LLM/provider、daemon/process 起動、ファイル書込みを行わない | 同名、同上 | true |
| `doctor_masks_filter_and_secret_values` | filter command、API key、credential、log 本文を出さない | 同名、同上 | true |
| `doctor_checks_continue_after_failure` | 一 check の FAIL 後も独立 local checks を返す | 同名、同上 | true |
| `doctor_protocol_check_uses_existing_ping_contract` | 1 回の既存 Ping/Pong 観測だけを socket/protocol checks で共有する | 同名、同上 | true |
| `status_legacy_output_remains_compatible` | status の fields、3 format、socket failure 時 exit 0 を維持する | 同名、同上 | true |
| `doctor_tsv_env_remain_machine_readable` | legacy keys を維持し安定 check keys を追記する | 同名、同上 | true |

## 3. 変更対象クレート / ファイル

| クレート | ファイル | 変更内容 |
|----------|----------|----------|
| `ai` | `ai/src/domain/reports.rs` | `CheckStatus`、`HealthCheck`、`DoctorReport` と severity 集約、human / JSON / TSV / env render を追加する。既存 `DiagnosticsReport` は status 正本として保持する |
| `ai` | `ai/src/domain/mod.rs` | 新しい診断ドメイン型を export する |
| `ai` | `ai/src/clap_cli.rs` | `Doctor.format` だけ `Option<OutputFormatArg>` にし、未指定と明示 `tsv` を区別する。共有 enum の default と `Status` / `Ping` は変えない |
| `ai` | `ai/src/main.rs` | `Status` と `Doctor` の dispatch を分離し、doctor report 構築と exit code を接続する。`resolve_shell_log_info`、`resolve_filter_metadata`、`resolve_tools`、`ping_detailed` の既存規則を再利用する |
| `ai` | `ai/tests/0058_aish_doctor_diagnostics.rs` | 全新規 AC の fixture 付き統合テスト。Scope Lock 時点の ignored skeleton を置換する |
| `ai` | `ai/tests/phase_a_cli.rs` | 既存 `doctor_alias_uses_doctor_command_name` を維持。必要なら新 JSON schema の assertion に同期するが、関数名・registry row は変えない |
| docs | `docs/architecture.md` | `ai doctor` の domain model、6 checks、Ping 共有、出力 / exit 契約を同期する |
| docs | `docs/manual/ai-doctor-diagnostics.md`、`docs/manual/README.md` | mock socket と一時 HOME を使う手動確認手順、秘密値非露出確認を追加する |
| scripts | `scripts/smoke-mock.sh` | mock aibe に対する human と JSON doctor、exit 0 の smoke を追加する |
| registry | `scripts/spec-acceptance.toml`、`scripts/feature-scope.toml`、`docs/0000_spec-index.md` | 実装進捗に合わせ pending / 状態を更新する |

`aibe`、`aish`、`aibe-protocol` は変更しない。新 RPC / DTO を追加しない。

## 4. モデルと責務配置

`CheckStatus`、`HealthCheck`、`DoctorReport` は副作用を持たない表示・判定モデルなので `ai/src/domain/reports.rs` に置く。`CheckStatus` の wire 値は `ok|warn|fail`、総合 status は `FAIL > WARN > OK` の最大深刻度とする。`HealthCheck` は `id/status/message/suggestion`、`DoctorReport` は `command/status/checks` を持つ。OK の suggestion は `None` 可、WARN/FAIL は必須とし、constructor または builder で不変条件を集中させる。

I/O 観測は `main.rs` の CLI composition 経路に留め、ドメイン型へ filesystem / socket API を持ち込まない。実装が肥大化する場合は `ai/src/application/doctor.rs` に「既に取得した観測から checks を組み立てる」純粋な service を置いてよいが、新しい汎用 diagnostics framework や trait 群へ拡張しない。

## 5. CLI 分岐と出力契約

- `AiCommand::Status { format: OutputFormatArg, .. }` は現状の `run_diagnostic_command("status", ...)` 相当を維持し、必ず明示 / default の structured format を使う。
- `AiCommand::Doctor { format: Option<OutputFormatArg>, .. }` は専用 `run_doctor_command` へ dispatch する。`None` のみ human、`Some(Json/Tsv/Env)` は machine-readable とする。
- `OutputFormatArg` 自体や `Status`、`Ping`、その他 command の既定値を変更しない。
- `--quiet` は stderr の補助説明だけを抑止し、stdout report と exit code を変えない。JSON stdout に前後説明を混ぜない。
- doctor は checks に FAIL があれば `ExitCode::FAILURE`（1）、OK/WARN のみなら 0。config parse や report 構築不能など command 自体のエラーは既存 main error 経路（2 以上）に委ねる。

human は先頭に総合 status と `doctor` を表示し、固定順の各 check に status と message、WARN/FAIL の次行または同一 block に suggestion を出す。表示文言の完全一致より、status / ID / suggestion の構造をテストする。

JSON は次の schema だけを report として stdout に一つ出す。

```json
{"command":"doctor","status":"warn","checks":[{"id":"socket_reachable","status":"ok","message":"...","suggestion":null}]}
```

TSV / env は従来 `DiagnosticsReport` の全 key を削除・改名せず先に出し、次を追記する。

- TSV: `doctor.status`、各 check の `check.<id>.status`、`check.<id>.message`、`check.<id>.suggestion`
- env: `AI_DOCTOR_STATUS`、各 check の `AI_DOCTOR_CHECK_<UPPER_ID>_STATUS`、`..._MESSAGE`、`..._SUGGESTION`

値の escaping は既存 `append_tsv_row` / `append_env_line` を必ず使う。suggestion がない場合は空文字列とする。

## 6. 6 checks の判定ロジック

checks は以下の順で必ず一度ずつ構築する。

1. `socket_reachable`: `--socket` 優先、なければ `AiConfig.socket_path`。接続して request を送受信できない（不存在、permission、timeout を含む）なら FAIL。接続と応答受信まで成立した不正 response は socket 自体は OK とする。
2. `session_context`: `AISH_SESSION_DIR` と `implicit_session_id_from_env` を既存規則で解決する。両方が妥当なら OK、通常利用に必須でない未設定は WARN。不正 UTF-8 / basename 解決不能など利用不能は WARN とし、次の確認手順を suggestion にする。
3. `shell_log_readable`: `resolve_shell_log_info` / `resolve_shell_log_for_ask` の選択結果を使う。選択 path が存在し read 可能なら OK。明示 `AI_ASK_LOG` の解決失敗・存在するが read 不可は FAIL。log を利用しない選択または暗黙候補なしは WARN。本文は読んでも出力せず、可能なら metadata/open のみで可読性を判定する。
4. `tools_configuration`: `resolve_tools(None, &cfg.ask_tools)` で token/category 展開を検証する。成功かつ非空は OK、成功だが空集合は WARN、不正 token/category/`none` 混在は FAIL。message は有効 tool 名または件数だけに限定する。
5. `output_filter_configuration`: `AiConfig::load_for_diagnostics` で設定ファイルの read/parse 成否を観測し、失敗時は FAIL（suggestion 必須）。FAIL message は filter 単体障害ではなく、ai config 全体の読込または parse 失敗であることが分かる表現にする（例: `ai config could not be read or parsed; output filter configuration cannot be verified`）。成功時は `resolve_filter_metadata(AI_FILTER, None, cfg.ask_filter)` の `enabled/source/masked` を使う。未設定は利用可能な通常状態として OK、設定済みかつ source が解決し masked なら OK。通常コマンド向けの `AiConfig::load()`（fail-open）は変更しない。filter command 本文は絶対に含めない。
6. `protocol_compatibility`: 下記共有 Ping 観測が正しい `ClientResponse::Pong` として decode できれば OK。不正応答 / decode failure は FAIL。数値 version は表示しない。

各 FAIL 後も依存しない local checks を続行する。依存観測が取れない check は推測で OK にせず、設計書の範囲で WARN / FAIL と理由・suggestion を返す。

## 7. Ping 一回共有

`ping_detailed` の bool / error だけでは「接続不能」と「接続済み不正応答」を分離できない場合、既存 Ping request / Pong decode を一回だけ行う doctor 専用の小さな観測型（例: `PingObservation::{Pong, Unreachable, InvalidResponse}`）を CLI / adapter 側に追加する。`socket_reachable` と `protocol_compatibility` は同じ値を参照し、二度接続しない。

| 観測 | socket_reachable | protocol_compatibility |
|------|------------------|------------------------|
| Pong decode 成功 | OK | OK |
| 接続不能 / timeout | FAIL | FAIL |
| 接続後の不正応答 / decode failure | OK | FAIL |

request は既存 `ClientRequest::Ping`、response は既存 `ClientResponse::Pong` を使う。`ensure_running`、daemon 起動、新 RPC、protocol version DTO は禁止する。

## 8. Fixture 方針

統合テストは `phase_a_cli.rs` の `MockSocketServer` / `write_ai_config` の作法を共通 helper 化または複製して、実 daemon と外部 API に依存させない。各 test は一時 HOME、明示 `AI_CONFIG`、一時 socket、`env_remove` 相当で環境を完全に制御する。

- healthy fixture: Pong mock、妥当 config、有効 tools、session dir、readable `log.jsonl`、秘密値を含む filter 設定。
- warning fixture: socket は Pong、session / log / tools が任意未設定。FAIL なしを保証する。
- unreachable fixture: 存在しない socket。local checks が後続することも確認する。
- invalid-response fixture: 接続後に Ping 以外 / malformed JSON を一度返し、socket OK + protocol FAIL と request count 1 を確認する。
- unreadable / explicit-log fixture: Unix permission test は root 実行でも決定的になるよう、directory/path shape または resolver error fixture を優先する。
- mutation guard: temp tree の before/after snapshot、mock request log、子 process を起動できない PATH fixture 等を組み合わせ、Ping 以外の RPC、ファイル作成・更新、process 起動がないことを検証する。
- secret fixture: `AI_FILTER`、config 内 filter、API key 風文字列、credential 風文字列、log 本文 marker を stdout/stderr の双方に含まないことを検証する。

## 9. テスト計画

統合テスト関数名は AC ID と完全一致させる。単体テストは補助的に追加してよいが registry の代替にはしない。

| 種別 | 対象 | 主な検証 |
|------|------|----------|
| domain unit | `ai/src/domain/reports.rs` | severity 順序、WARN/FAIL suggestion、固定 render keys、TSV/env escaping、human 表示 |
| CLI parse unit | `ai/src/clap_cli.rs` | doctor 未指定 `None`、明示 3 format、status default TSV |
| integration | `ai/tests/0058_aish_doctor_diagnostics.rs` | 上表の新規 10 AC。binary exit/stdout/stderr、fixture、Ping count を検証 |
| regression | `ai/tests/phase_a_cli.rs` | 既存 alias AC と status / ping 周辺既存テスト |
| smoke | `scripts/smoke-mock.sh` | mock aibe 上の `ai doctor --quiet` human と `--format json`、両方 exit 0 |

## 10. 非目標・触ってはいけないもの

- `--full` / `--network`、provider / LLM / internet connectivity、LLM dry-run
- `--fix-plan` / `--fix`、daemon start/restart、config / session / log 書込み
- 独立 `aish doctor`、PATH / duplicate install / binary version 探索
- replay、smart observation、safe-write、handoff、MCP の診断
- client/server 数値 protocol version、新 wire DTO / RPC
- background watcher、state machine、永続 aggregate、pack composition、汎用 plugin / diagnostics framework
- `aibe` / `aish` / `aibe-protocol` の変更、`ai status` の deprecate、既存 DiagnosticsReport key の削除・改名

## 11. 完了定義

1. 0058 の全 AC が fixture 付き本番経路テストになり、`#[ignore]` を外して `pending=false` になっている。
2. `cargo test -p ai -j 1 --test 0058_aish_doctor_diagnostics` と既存 `phase_a_cli` の関連テストが成功する。
3. `docs/architecture.md`、manual、index が実装と同期する。
4. `./scripts/check-feature-scope.py` と `./scripts/check-spec-acceptance.py` が成功する。
5. 完了直前に `./scripts/verify.sh` を成功させ、`.verify-timing-last` の timing summary を報告する。
6. 続けて `./scripts/smoke-mock.sh` を成功させる。実 API key は使わない。
7. 手動検証を実施した場合は結果を報告し、未実施なら残リスクとして明記する。
8. 全条件達成後だけ本書を `docs/done/` へ移し、index を「設計確定（実装済み）」へ更新する。

## 12. 仕様との差分

なし。

