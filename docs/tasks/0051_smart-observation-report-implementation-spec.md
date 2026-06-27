# 0051 — Smart Preprocessor Observation Report 実装指示書

## 0. 目的

設計書 0051 に従い、ai smart stats / recent / report を本番経路へ追加する。

## 1. パック構成の適用

No。軽量な read-only CLI と reader に閉じ、optional 機能束を構成しない。

## 2. Phase

| Phase | 内容 | ゲート |
|---|---|---|
| 1 | DTO、bounded tail reader、filter、stats、renderer | unit tests |
| 2 | clap と main dispatch、CLI integration | CLI tests |
| 3 | architecture/security/testing/manual と registry | verify |

## 3. 受け入れ条件

| ID | 条件 | テスト |
|---|---|---|
| reader_resilience | 不正行を数え正常行を維持 | reader_counts_invalid_lines_and_keeps_valid_records |
| stats | distribution/count/latency 集計 | stats_aggregates_distributions_counts_and_latency |
| filters | session/since filter | filters_session_and_since_hours |
| stats_json | stats JSON CLI | smart_stats_json_cli_reports_invalid_lines |
| stats_tsv | stats TSV CLI | smart_stats_tsv_cli_uses_key_value_rows |
| recent_safe | recent は既知フィールドのみ | smart_recent_json_cli_outputs_only_known_safe_fields |
| report_safe | Markdown と raw 非出力 | smart_report_cli_outputs_markdown_without_raw_user_text |

全件 pending=false とし、実装指示書はコミット時に docs/done へ移す。

## 4. 仕様との差分

なし。
