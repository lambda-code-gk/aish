# テスト方針

aish ワークスペースのテスト種別、置き場所、実行方法。機能変更時は **テストとこの文書を同じコミットで更新** する。

## 実行コマンド（標準）

```bash
./scripts/verify.sh
```

### Rust ツールチェーン

`rust-toolchain.toml` で rustc / rustfmt / clippy のバージョンを固定している（現行: 1.92.0）。
更新するときは channel を上げ、`./scripts/verify.sh` が緑になることを確認してからコミットする。

TTY の影響を避けたいときは、非TTYラッパーを使う。

```bash
./scripts/verify-non-tty.sh
```

個別に回す場合:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo build -p aibe   # aibe-client 統合テストが spawn するバイナリ
cargo test --workspace --exclude aibe-client
cargo test -p aibe-client -- --test-threads=1
./scripts/check-architecture.sh   # クレート境界 + subprocess 方針 + check-hexagonal.sh
python3 scripts/check-hexagonal-effects.py   # effect boundary（verify 経由でも実行）
./scripts/check-docs-consistency.sh   # README / 仕様索引 / testing.md 表の実在パス
```

ローカルでも PR 前に `verify.sh` を通す。GitHub Actions（[`.github/workflows/ci.yml`](../.github/workflows/ci.yml)）の `verify` job も同じ。

### 二段階検証

開発中のフィードバックと完了判定を分離する。

1. **実装中**: 変更箇所に対応する最小のcheck/testを実行する
2. **完了直前**: `./scripts/verify.sh`を1回実行する
3. 全体ゲートが失敗したら、失敗した検査だけで修正を回し、最後に`verify.sh`を再実行する
4. **`verify.sh` 実行後**: AI エージェントは `.verify-timing-last`（またはサマリー部分のみ）を読み、ユーザー向け返信に転記する。cargo test 全文はコンテキストに載せない（失敗時は失敗箇所のみ追加）

targeted検証は開発を速くするための補助であり、DoDやCIの代替ではない。対象は自動推測せず、パッケージまたは静的検査を明示する。

```bash
./scripts/verify-targeted.sh --package aibe
./scripts/verify-targeted.sh --package ai --test ask_integration
./scripts/verify-targeted.sh --docs
./scripts/verify-targeted.sh --architecture
./scripts/verify-targeted.sh --codex-tooling
```

`--test`はCargoのintegration test target名で、複数回指定できる。パッケージ検証では`cargo fmt --all -- --check`、対象パッケージの`cargo clippy -p <pkg> -- -D warnings`、対象テストを順に実行する。`aibe-client`は先に`aibe`をbuildし、テストへ`--test-threads=1`を付ける。すべてのcargo testは`-j 1`で実行する。

| 変更対象 | 実装中の推奨検証 |
|----------|------------------|
| `aibe/` | `--package aibe`、必要なら`--test <integration-test>` |
| `ai/` | `--package ai`、必要なら`--test <integration-test>` |
| `aish/` | `--package aish`、必要なら`--test <integration-test>` |
| `aibe-client/` | `--package aibe-client` |
| `aibe-protocol/` | `--package aibe-protocol`に加え、影響する直接利用クレートも個別確認 |
| `aish-replay/` | `--package aish-replay` |
| shell script | `bash -n <script>`と対応する専用チェック・テスト |
| docsのみ | `--docs` |
| 依存・レイヤー・副作用境界 | `--architecture` |
| Codex統合 | `--codex-tooling` |

複数クレートへ影響する変更は、同じコマンドに推測させず、影響するパッケージごとに順番に実行する。

### メモリ不足環境（低 RAM・AI エージェント）

OOM を避けるため、**既定は直列**とする。

| 手段 | 内容 |
|------|------|
| `./scripts/verify.sh` | 既定: `CARGO_BUILD_JOBS=1`、`cargo test -j 1`（`aibe-client` は `--test-threads=1` 維持） |
| 手動 `cargo test` | **`-j 1`** を付ける。複数クレートのテストを **同時に走らせない** |
| 並列に戻す | RAM に余裕があるマシンのみ `VERIFY_PARALLEL=1 ./scripts/verify.sh`（**CI の verify job は既定で並列**） |

### CI と smoke の役割分担

| 手段 | 内容 |
|------|------|
| **`verify` job** | `./scripts/verify.sh`（`VERIFY_PARALLEL=1` でコンパイル並列化。`aibe-client` は直列テスト。`| tail` で包むと無出力に見えるので避ける） |
| **`smoke-mock` job** | [`scripts/smoke-mock.sh`](../scripts/smoke-mock.sh) — 実 binary・実 socket・`provider = "mock"` で `ai ask` 1 回（verify の rust-cache を `shared-key: ci` で復元） |
| **`feature-off` job** | `cargo build/test --workspace --no-default-features` — memory feature off（basic pack）のビルド・テスト退行検出 |
| **ローカル smoke** | `./scripts/smoke-mock.sh`（CI と同じ。実 API キー不要） |

`cargo test --workspace` はロジック・プロトコル・モック統合（例: `ai/tests/ask_integration.rs`、`aibe/tests/ai_ask_e2e.rs`）を広く網羅する。smoke は **CLI の `stdout` / `stderr` 契約** と設定ファイル参照・プロセス起動順を、テストでは拾いにくい経路で固定する。smoke は `cargo test` の代替ではない。

### 0067 recall keybinding PTY 回帰

`ai/tests/0067_recall_keybinding_tty_restore.rs` は、実 PTY の bash / zsh × `Alt+.` / `Alt+,` で成功・空・cache 不在・subprocess 非 0・連続 shortcut を labeled matrix として実行する。shortcut は完全な `ESC .` / `ESC ,`、cursor / history は完全な CSI を一括送信し、cursor 位置へ marker を挿入して実行した結果と、履歴を確定して実行した結果で line editor の状態を判定する。固定 sentinel prompt が再び観測された stable prompt 間で `c_iflag` / `c_oflag` / `c_cflag` / `c_lflag` / `c_cc` を比較し、入力待ち中の一時的な raw / cbreak は比較しない。

同ファイルの non-PTY shell 統合は PATH 上の deterministic stub `ai` が stdin を read し、成功・空・非 0 の全経路で EOF となることと buffer の更新 / 非破壊を確認する。0067 の Human Shell AC は `ai/tests/0055_collaborative_handoff_vertical_e2e.rs` の outer-PTY E2E で `CollaborativeMockServer`、実 `ai`、実 `aish human-shell`、製品 `PtyShell::run_shell` を通し、bash / zsh の通常 CSI cursor と `Alt+.` 後の CSI cursor がコマンドを意図した位置で編集することを確認する。`aish/tests/0067_recall_keybinding_tty_restore.rs` は一時 rcfile 単体の両 shortcut、cursor・上下 history・termios、候補なし時の binding 所有を補助回帰として維持する。`aish/tests/0067_aish_shell_alt_period_csi.rs` は実 `aish shell`（replay DEBUG）で `Alt+.` 直後の CSI が caret 挿入にならないことを確認する。CI 前提の bash / zsh が無い場合は skip せず失敗し、`feature-off` job も zsh を install する。いずれも 0055 / 0057 と同じ `openpty`、controlling TTY、期限付き read、子 process cleanup の形を局所的に再利用し、汎用 PTY framework は追加しない。

### 0038 Phase D basic build（feature matrix）

設計: [spec/0038_contextual-memory-pack-phase-d-spec.md](spec/0038_contextual-memory-pack-phase-d-spec.md)。

| コマンド | 目的 |
|----------|------|
| `./scripts/verify.sh` | default build（`memory` feature on）の fmt / clippy / test / architecture / docs |
| `./scripts/smoke-mock.sh` | default build の CLI + mock aibe 導通（memory ルート含む） |
| `cargo build --workspace --no-default-features` | basic build がリンクできること |
| `cargo test --workspace --no-default-features` | feature off 時に memory 統合テストがスキップされ、残りが通ること |

`memory` feature off 時、contextual memory の integration test は crate 先頭の `#![cfg(feature = "memory")]` で除外される（例: `aibe/tests/work_rpc.rs`）。unit test は `plugin_memory` 内の `#[cfg(test)]` が同 feature に従う。

### dev プロファイル（ビルドサイズ）

ルート `Cargo.toml` の `[profile.dev]` / `[profile.test]` で依存クレートの debug info を抑え、`target/` の肥大化を防ぐ（`debug = "line-tables-only"` + `[profile.*.package."*"] debug = false`）。gdb/lldb で変数を詳細に見る必要があるときは一時的に `debug = 1` に戻す。

### 0039 AISH pack externalization

設計: [spec/0039_aish-memory-pack-externalization-spec.md](spec/0039_aish-memory-pack-externalization-spec.md)。

| 観点 | テスト位置 |
|------|------------|
| baseline kind pack（`kinds.toml`） | `aibe/src/domain/memory_kind_registry.rs`（unit）、`aibe/src/adapters/outbound/filesystem_memory_kind_registry.rs`（unit） |
| `kind_files = []` / compat mode | `filesystem_memory_kind_registry.rs` の `explicit_empty_kind_files_yields_no_kinds` |
| baseline recipe pack | `aibe/src/domain/memory_recipe_registry.rs`（unit）、`filesystem_memory_recipe_registry.rs`（unit） |
| generic recipe CLI 回帰（`ai mem run <recipe>`） | `aibe/tests/memory_recipe.rs`、`ai/tests/phase_a_cli.rs` |
| config `kind_files` / `recipe_files` / `feature_files` パス解決 | `aibe/src/adapters/outbound/toml_config.rs`（unit） |
| `ai` feature off で `memory_space_id` 解決なし | `cargo build -p ai --no-default-features`（compile-time gate） |

### 0046 aibe graceful restart

設計: [spec/0046_aibe-graceful-restart-spec.md](spec/0046_aibe-graceful-restart-spec.md).

| 観点 | テスト位置 |
|------|------------|
| PID file の read/write、識別子検証、stale 判定 | `aibe/src/daemon/pid_file.rs`（unit）、`aibe/src/adapters/inbound/control_plane.rs`（unit） |
| `aibe status --format json` の必須フィールド | `aibe/tests/graceful_restart.rs`（integration） |
| `aibe stop` / `aibe restart` が mock daemon に対して正しく動くこと | `aibe/tests/graceful_restart.rs`（integration） |
| `MemorySubscribe` / active turn shutdown | `aibe/tests/memory_subscribe.rs`、`aibe/tests/agent_turn_tools.rs` 等の既存統合観点に追加 |
| mock 正常系の end-to-end 確認 | `./scripts/smoke-mock.sh` もしくは同等の temp HOME / mock aibe スモーク |

### 0041 / 0042 Smart Feature Plan

設計: [spec/0041_ai-smart-feature-plan-spec.md](spec/0041_ai-smart-feature-plan-spec.md)、[spec/0042_configurable-smart-features-spec.md](spec/0042_configurable-smart-features-spec.md)、[spec/0043_feature-pack-boundary-hardening-spec.md](spec/0043_feature-pack-boundary-hardening-spec.md)。

| 観点 | テスト位置 |
|------|------------|
| `FeatureAction` serde / `feature_actions` 省略互換 | `aibe-protocol/src/response.rs`（unit） |
| `feature_executor` safe tool 判定 / log tail clamp / history summary 分離 | `ai/src/application/feature_executor.rs`（unit）、`ai/tests/smart_feature_plan.rs`（unit） |
| `route_turn` プロンプト schema / registry trigger マージ / memory_query 重複防止 | `aibe/src/domain/feature_registry.rs`（unit）、`aibe/tests/route_turn.rs`（integration） |
| `ai ask` smart feature 導通（route_turn → feature_executor → agent_turn） | `ai/tests/smart_feature_ask_e2e.rs`（integration、TTY + mock aibe） |
| `retry` / `rerun` の smart route 再評価（ask + TTY） | `ai/src/main.rs`（unit: `payload_eligible_for_smart_rerun`）、手動: `docs/manual/ai-smart-entry.md` §8 |
| baseline `features.toml` 読み込み | `aibe/memory/packs/aish-memory/features.toml`、`FilesystemFeatureRegistryLoader` |
| `memory.enabled=false` で feature registry empty / route_turn strip | `aibe/src/application/server.rs`、`aibe/tests/memory_disabled.rs` |
| `RoutePlan.log_tail_bytes` top-level clamp | `aibe/src/application/route_turn.rs`（unit） |
| `feature_files=None` baseline / `feature_files=[]` empty | `aibe/src/adapters/outbound/filesystem_feature_registry.rs`（unit）、`aibe/src/ports/outbound/config.rs`（`resolve_feature_pack` unit） |
| generic memory（`kind_files=[]` + `recipe_files=[]` + `feature_files=None`）で feature empty | `aibe/tests/feature_pack_boundary.rs` |
| `FeaturePackConfig` / `EffectiveFeatureMode` 解決（0043 Phase 3） | `aibe/src/domain/feature_registry.rs`、`aibe/src/ports/outbound/config.rs`（unit）、`aibe/tests/feature_pack_boundary.rs` |
| feature eligibility（`requires_memory` / `requires_recipe`） | `aibe/src/domain/feature_registry.rs`（unit）、`aibe/tests/route_turn.rs` |
| `recommended_tools` read-only 統一（`shell_exec` 除外） | `aibe-protocol/src/tool_name.rs`（unit）、`aibe/src/application/route_turn.rs`（unit）、`aibe/tests/route_turn.rs`、`ai/tests/smart_feature_plan.rs` |
| 手動 smart feature plan | [`docs/manual/ai-smart-entry.md`](manual/ai-smart-entry.md) §8 |

### 0044 Smart Preprocessor / Local Intent Router

設計: [spec/0044_smart-preprocessor-spec.md](spec/0044_smart-preprocessor-spec.md)。受け入れ条件 ↔ テスト: [`scripts/spec-acceptance.toml`](../scripts/spec-acceptance.toml)（`./scripts/check-spec-acceptance.py`）。

| 観点 | テスト位置 | 備考 |
|------|------------|------|
| feature hashing / classifier / confidence gate / redaction | `ai/src/domain/smart_preprocessor.rs`（unit） | |
| observation log（secret 非保存） | `ai/src/adapters/outbound/smart_preprocessor_observation.rs`（unit） | |
| shadow / assist / gate mode 導通 | `ai/tests/smart_preprocessor_ask_e2e.rs`（integration、TTY + mock aibe） | |
| gate 短絡（route_turn 省略） | `ai/tests/smart_preprocessor_ask_e2e.rs` | `gate_mode_skips_route_turn_for_simple_chat` |
| memory write / safety head / feature redaction | `ai/src/domain/smart_preprocessor.rs`（unit） | |
| non-TTY は preprocessor / route_turn を飛ばす | `ai/tests/phase_a_cli.rs` | |
| `--preset` / `--tools` / `--yes-exec` 明示値の優先 | 既存 smart entry / smart feature テスト | preprocessor 専用 AC は未登録 |

**Phase 2.6**（threshold 分離・observation 拡張・bundled model）:

| 観点 | テスト位置 | 備考 |
|------|------------|------|
| `assist_threshold`（既定 0.55）と `recent_summary` 注入の一致 | `assist_threshold_injects_error_summary_hint` | `ai/src/domain/smart_preprocessor.rs` |
| `route_turn_threshold` 未満では gate 短絡しない | `assist_and_route_turn_thresholds_are_distinct` | domain unit |
| `reason_codes` / `failure_kind` / `context_needs` / `tool_hints` の observation 保存 | `observation_persists_reason_codes` 等 | `smart_preprocessor_observation.rs` |
| bundled model（`model_path` 未指定） | `bundled_model_is_used_when_model_path_is_missing` | `smart_preprocessor_model.rs` |
| `session_error` feature prefix | `session_error_summary_uses_session_error_prefix` | observation unit |

Phase 2.9（local route fast path）:

| 観点 | テスト位置 | 備考 |
|------|------------|------|
| `LocalRouteDecision` の deterministic 導出 | `local_route_decision_is_deterministic` | `ai/src/domain/smart_preprocessor.rs` |
| CLI 上限つき tool enablement | `local_route_enabled_tools_are_clamped_to_cli_allowlist` | `ai/src/main.rs` |
| high confidence safe input で `route_turn` 省略 | `local_route_skips_route_turn_for_high_confidence_safe_input` | `smart_preprocessor_ask_e2e.rs` |
| unsafe / medium は `route_turn` fallback | `local_route_falls_back_to_route_turn_for_medium_or_unsafe_input` | 同上 |
| observation metrics（skip / fallback / latency / tokens saved） | `local_route_observation_records_metrics` | `smart_preprocessor_observation.rs` |
| observation 3軸（`route_turn_required` / `short_circuit_allowed` / `inject_hints`） | `local_route_observation_records_metrics` 等 | 同上 |
| route kind 5種の導出 | `local_route_kind_derivation_covers_phase_targets` | `smart_preprocessor.rs` |
| context / output_style の fast path 配線 | `apply_local_route_wires_context_and_output_style_messages` | `ai/src/main.rs` |

### 0051 Smart observation report

| 観点 | テスト位置 |
|------|------------|
| 空・欠損・不正行・未知フィールド・末尾 limit | ai/src/adapters/outbound/smart_observation_log_reader.rs |
| distribution / count / latency / filter | ai/src/domain/smart_observation_report.rs |
| stats/recent/report CLI と raw 非出力 | ai/tests/smart_observation_cli.rs |

### 0052 ai work

設計: [spec/0052_ai_work.md](spec/0052_ai_work.md)。受け入れ条件は [`scripts/spec-acceptance.toml`](../scripts/spec-acceptance.toml) で Phase ごとに管理する。

| Phase 0 観点 | テスト位置 |
|--------------|------------|
| Work request / response DTO、unknown field 拒否 | `aibe-protocol/src/work.rs` |
| atomic snapshot、permission、破損非上書き、並行 mutation | `aibe/src/adapters/outbound/work_store.rs` |
| 実 WorkStore を使う空 query | `aibe/tests/work_rpc.rs` |
| BasicPack の Work RPC 拒否 / injection no-op | `aibe/src/application/basic_memory_pack.rs` |
| 全 Work subcommand parse | `ai/src/clap_cli.rs` |
| dashboard / status / list の空表示 | `ai/tests/work_cli.rs` |
| runtime disabled / feature-off | `ai/tests/memory_disabled_cli.rs`、`ai/src/application/memory_stub.rs` |

| Phase 1 観点 | テスト位置 |
|--------------|------------|
| start、旧active pause、focus、idea/note/decision、defer | `aibe/tests/work_rpc.rs` |
| populated dashboard/status/listのsection・分類 | `ai/tests/work_cli.rs` |
| apply RPC、人間向けstdout、protocol errorのstderr/non-zero | `ai/tests/work_cli.rs` |

| Phase 2 観点 | テスト位置 |
|--------------|------------|
| switch、finish、active 必須 error、missing / done / stack guard | `aibe/tests/work_rpc.rs` |
| switch / finish の apply RPC 人間向け stdout | `ai/tests/work_cli.rs` |

| Phase 3 観点 | テスト位置 |
|--------------|------------|
| push、nested push、pop、empty stack、child entry 非 merge | `aibe/tests/work_rpc.rs` |
| status / list の stack marker | `ai/tests/work_cli.rs` |
| push / pop の apply RPC 人間向け stdout | `ai/tests/work_cli.rs` |

Phase 4 の AC は実装済みで、`pending = false` と `#[ignore]` 解除済みの状態を維持する。feature-off は `cargo test -p ai -j 1 --no-default-features --lib work_cli_stub_rejects_when_memory_feature_is_disabled` でも確認する。手動手順は [manual/ai-work.md](manual/ai-work.md)。

Phase C で追加した `chat` / `--progress` / streaming / cancel / `--timeout` / `--yes-exec` は、主に統合テストと [`docs/manual/ai-ux.md`](manual/ai-ux.md) で確認する。`chat` の transcript は `ai` 側で成功 turn ごとに追記し、`history` payload の `request_messages` に保存する。`retry` / `rerun` は payload に transcript があればそれを再生する。`--yes-exec` は [`ai/tests/yes_exec_integration.rs`](../ai/tests/yes_exec_integration.rs) で非 TTY 含め検証する。history GC は `history_max_entries`（既定 500、`0` で無効）で [`local_history.rs`](../ai/src/adapters/outbound/local_history.rs) が prune する。streaming の multi-delta forward は [`aibe/tests/agent_turn_streaming.rs`](../aibe/tests/agent_turn_streaming.rs) を参照する。

### 0017 以降のクレート別テスト配置

| クレート | 単体 | 統合 / E2E |
|----------|------|------------|
| **aibe-protocol** | `ClientRequest` / `ClientResponse` / `ToolName` の serde（crate 内 `#[cfg(test)]`） | — |
| **aibe-client** | socket 往復の契約固定（`route_turn` / `agent_turn` 承認 prompt → approval → final、TTY 非依存） | `transport.rs`、`tests/agent_turn_approval.rs`、`tests/route_turn.rs`、`client_ping.rs`、`ensure_running_*.rs` |
| **ai** | 承認 UI: `y/n/a/c`、tier、非対話 fail-closed、escape 表示 | `adapters/outbound/shell_exec_approval_ui.rs`、`domain/shell_exec_approval.rs`（`#[cfg(test)]`） |
| **ai** | `--yes-exec` / session cache / legacy cache 互換 | `adapters/outbound/yes_exec_cache.rs`、`tests/yes_exec_integration.rs` |
| **ai** | `shell_exec` 承認 UX の tier / pattern / choice 正本 | `tests/shell_exec_approval_ux.rs` |
| **aish** | セッション prune 順序・CLI 引数 | `session_store.rs`、`tests/session_cli.rs` |
| **ai** | `--session` hex 検証・presenter / allowlist / output filter | `adapters/outbound/shell_log_resolver.rs`、`output_filter.rs`、`stdout_presenter.rs`、`ask_integration.rs` |
| **ai** | local history / retry / rerun / preset / log-tail / chat transcript / smart entry / exit codes / yes-exec / history GC | `application/history.rs`、`adapters/outbound/local_history.rs`、`tests/history_cli.rs`、`tests/ux_gap_closure.rs`、`tests/yes_exec_integration.rs`、`src/main.rs`、`tests/phase_a_cli.rs` |
| **aibe** | assistant streaming forward | `tests/agent_turn_streaming.rs`、`adapters/outbound/scripted_mock_llm.rs` |
| **aibe** | contextual memory pack 境界（0038 Phase B） | `application/basic_memory_pack.rs`（unit）、`plugin_memory/`（Phase D、`#[cfg(feature = "memory")]`）、`tests/memory_pack_turn_hook.rs`、`tests/memory_disabled.rs`（basic 拒否回帰）、`tests/contextual_memory.rs` / `tests/memory_subscribe.rs`（contextual 回帰） |
| **ai** | contextual memory CLI policy pack（0038 Phase C） | `plugin_memory/`（Phase D）、`application/memory_*`（facade）、`tests/phase_a_cli.rs`（`mem add` / `mem kinds` / 専用 CLI 回帰）、`tests/memory_disabled_cli.rs`（disabled gate 回帰） |
| **aibe** / **ai** | compile-time packaging（0038 Phase D） | `cargo build --workspace --no-default-features`、`cargo test --workspace --no-default-features`（memory 統合テストは `#[cfg(feature = "memory")]` で default build のみ） |
| **aibe** | server / agent / tools / 承認 / 外部コマンド（0026 の shell_exec 経路） | `socket_protocol.rs`、`agent_turn_loop.rs`、`shell_exec.rs`、`shell_exec_approval_socket.rs`、`external_commands.rs` |

## テスト種別

| 種別 | 目的 | 置き場所の目安 | 必須タイミング |
|------|------|----------------|----------------|
| **単体** | 純粋な関数・serde・状態機械 | 各クレート `src/` 内 `#[cfg(test)]` または `tests/` | ロジック追加時 |
| **統合** | クレート API・モック相手の I/O | `<crate>/tests/*.rs` | プロトコル・CLI 変更時 |
| **E2E** | 複数バイナリ/ソケット連携 | ワークスペース `tests/` または `aibe/tests` + フィクスチャ | 境界が動く変更時 |
| **手動** | 実ターミナル・実シェル・実 API | `docs/manual/*.md` に手順のみ | 自動化困難な体験確認時 |

## クレート別の期待

### aibe

- **単体**: JSON メッセージの serialize/deserialize、設定パース、allowlist、`agent_turn` ループ（`ScriptedMockLlm`）。ツール失敗は tool result で継続（allowlist 外 `shell_exec`、パス制限外 `read_file`、モデル幻覚ツール、subprocess 非ゼロ終了、`shell_exec` タイムアウト）。`shell_exec` の subprocess 制御（並行 stdout/stderr drain、タイムアウト時 kill/reap）は `aibe/src/adapters/outbound/tools/shell_exec.rs` の単体テスト（`run_subprocess` + 大量出力の非誤 timeout + PID `ESRCH` 検証）が正本
- **統合**: Unix socket で `ping` / `agent_turn`（ツールなし・`read_file` ループ）が完走
- **外部コマンド（0026）**: `aibe/tests/external_commands.rs` で fixture `echo` の正常系・allowlist 拒否を固定。`approval_source` の `external_command=` 付与は `shell_exec.rs` 単体テストが正本。CLI coding agent は first-class tool にしない。
- **E2E**: デーモン起動 → クライアント 1 リクエスト → 応答（ネットワーク不要な fixture 推奨）
- **手動**: 実プロバイダ + 実キーでの 1 ターン（`openai_compatible` / Gemini — `docs/manual/aibe-openai-compatible.md` 等）

### aish

- **単体**: ログ行フォーマット、イベント組み立て
- **統合**: 固定コマンド（`echo`, `false`）の実行とログファイル内容
- **手動**: 対話シェルで入出力がログに残ること（`docs/manual/aish-shell-log.md`）

### ai

- **単体**: 設定読み込み、ログ tail 抽出、history メタデータ、exit code 変換
- **統合**: モック aibe サーバへ接続して表示、non-TTY で `route_turn` を飛ばすこと、`--new` / `AI_SESSION_ID` の request 反映
- **E2E**: モック aibe + フィクスチャログで 1 セッション、`route_turn` / conversation store の往復
- **手動**: `ai` → 実 aibe → 表示（キーはユーザー環境のみ）。`route_turn` / `--new` / non-TTY fallback は [`docs/manual/ai-smart-entry.md`](manual/ai-smart-entry.md)、ツール allowlist は [`docs/manual/ai-ask-tools.md`](manual/ai-ask-tools.md)

### 0018 safe-tools-policy の検証観点

正式指示書: [0018_safe-tools-policy-spec.md](done/0018_safe-tools-policy-spec.md)。設計の上位正本は [architecture.md](architecture.md)。

| 種別 | ファイル | 担保する観点 |
|------|----------|--------------|
| **integration** | `ai/tests/tool_catalog_sync.rs` | `@read-only` / `@exec` / `@full` の展開。`@full` に `shell_exec` が含まれないこと |
| **integration** | `ai/tests/tool_names_sync.rs` | `KNOWN_TOOLS` とカテゴリ表の機械同期 |
| **integration** | `ai/tests/ask_integration.rs` | 起動時ツール表示・`shell_exec` 有効時の warning 文言 |
| **integration** | `aibe/tests/request_tool_validation.rs` | allowlist 外ツール・`cwd` 未送信などの server-side 拒否 |
| **integration** | `aibe/tests/agent_turn_loop.rs` | `agent_turn` ループと tool result 継続の入口 |
| **integration** | `aibe/tests/agent_turn_tools.rs` | safe tools / `shell_exec` を含むツール実行経路 |
| **integration** | `aibe/tests/socket_protocol.rs` | socket 経由の `agent_turn` と拒否応答 |
| **unit** | `aibe/src/adapters/outbound/tools/shell_exec.rs`（`#[cfg(test)]`） | `run_subprocess` の timeout / kill / reap、大量 stdout の非誤 timeout |
| **manual** | [manual/ai-ask-tools.md](manual/ai-ask-tools.md) | safe tools 表示、`shell_exec` の warning、拒否 / 承認の見え方 |

- **client / server の役割**: `ai` 側テストは allowlist 解決と起動時表示。`aibe` 側テストは実行時拒否とループ継続。client の warning だけに依存しないことは `aibe` の拒否テストで追う。
- **補完対象**: `shell_exec` が allowlist に含まれた場合の承認済み通過経路は、現状の統合テストだけでは十分に固定されていない。追加する場合は 0018 とは別の指示書で扱う。
- **0023 / 0036**: `ai` は `shell_exec_approval_ui` と `domain/shell_exec_approval` の unit で TTY/escape/tier/pattern を固定。`aibe-client/tests/agent_turn_approval.rs` で `approval_origin` 往復を固定。`ai/tests/shell_exec_approval_ux.rs` と `yes_exec_integration.rs` で cache / non-TTY を検証。pipe への `printf y` は [manual/ai-ask-tools.md](manual/ai-ask-tools.md) B3c で手動確認。
- **正本**: 検証計画の説明はこの文書に置き、運用手順は `docs/manual/ai-ask-tools.md` に寄せる。

### 0054 Safe File Write Tools の検証観点

設計: [spec/0054_safe-file-write-tools-spec.md](spec/0054_safe-file-write-tools-spec.md)。

| 種別 | ファイル | 担保する観点 |
|------|----------|--------------|
| **unit / integration** | `aibe-protocol/tests/0054_safe_file_write_red.rs` | `file:write` capability、`ToolApproval` wire、`ExecutedToolCall` audit 語彙 |
| **integration** | `aibe/tests/0054_safe_file_write_red.rs` | `write_file` / `apply_patch`、SHA-256 / stale_file、journal、socket 承認、§28 受け入れシナリオ |
| **integration** | `aibe/tests/file_change_service.rs` | `FileChangeService` prepare → approve → revalidate → journal → commit |
| **integration** | `aibe/tests/tool_approval_socket.rs` | `ConnectionApprovalGate` の write-like 承認往復 |
| **integration** | `ai/tests/0054_safe_file_write_red.rs` | `@edit` 展開、write tool 有効時 warning |
| **integration** | `ai/tests/file_write_approval_ui.rs` | stderr 承認 UI、non-TTY fail-closed、diff escape |
| **integration** | `aibe-client/tests/0054_safe_file_write_red.rs` | `ToolApprovalPrompt` callback 往復 |
| **manual** | [manual/ai-ask-tools.md](manual/ai-ask-tools.md) § D | `ai --tools @edit` での create / patch / stale / 拒否 |

受け入れ条件は `scripts/spec-acceptance.toml`（`spec = "0054"`）に Phase 1–9 で登録。

### 0035 memory identity split の検証観点

設計: [spec/0035_aibe-memory-identity-split-spec.md](spec/0035_aibe-memory-identity-split-spec.md)。

| 種別 | ファイル | 担保する観点 |
|------|----------|--------------|
| **unit** | `aibe-protocol/src/memory_space.rs` | `project_<hash>` / `legacy_session_*` の安定生成と path-safe 検証（`.` / `..` 拒否含む） |
| **unit** | `aibe/src/domain/memory_space.rs` | 解決順・`now` stale 判定 |
| **unit** | `aibe/src/application/agent_turn.rs`（`#[cfg(test)]`） | turn 注入が explicit `memory_space_id` に従うこと、cwd 無し turn でも注入されること |
| **integration** | `aibe/tests/contextual_memory.rs` | `sess_001`+`ctx_a` で set → `sess_002`+`ctx_a` で同じ goal、`sess_003`+`ctx_b` で分離、stale `now`、`memory_space_id` 未指定 request が落ちないこと |
| **integration** | `aibe/src/adapters/outbound/contextual_memory_store.rs`（`#[cfg(test)]`） | memory space 共有・分離、legacy data の lazy copy / read-through（元 store を壊さない） |
| **integration** | `ai/tests/phase_a_cli.rs` | memory RPC と agent_turn の両方に `memory_space_id` が載ること |
| **integration** | `ai/tests/context_cli.rs` | `ai context current/use/new`、env 優先、path-unsafe 名と壊れた config の拒否 |
| **unit** | `ai/src/application/memory_space.rs` | `AIBE_CONTEXT_ID` > config > project > legacy の優先順 |
| **manual** | [manual/contextual-memory.md](manual/contextual-memory.md) | `ai context` と sess/context マトリクス |

### 0037 Phase 4 MemoryRecipe の検証観点

設計: [spec/0037_aibe-contextual-memory-runtime-v1-spec.md](spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §8 / Phase 4。

| 種別 | 場所 | 内容 |
|------|------|------|
| **unit** | `aibe-protocol/src/memory.rs` / `request.rs` | `MemoryRecipeRun` / `MemoryRecipeProposalDto` roundtrip、unknown field 拒否 |
| **unit** | `aibe/src/domain/memory_recipe.rs` | LLM JSON 検証、markdown fence 拒否、`Add` のみ許可 |
| **integration** | `aibe/tests/memory_recipe.rs` | `ScriptedMockLlm` による clarify-goal、`apply=false` / `apply=true`、invalid output |
| **integration** | `ai/tests/phase_a_cli.rs` | `ai mem run <recipe>` の recipe id forwarding、`clarify-goal` 表示回帰、`--apply` 非対話 fail-closed |
| **manual** | [manual/contextual-memory.md](manual/contextual-memory.md) | `ai mem run clarify-goal` / `--apply` |

### 0037 Phase 5 MemorySubscribe の検証観点

| 種別 | 場所 | 内容 |
|------|------|------|
| **unit** | `aibe-protocol/src/memory.rs` / `request.rs` / `response.rs` | `MemorySubscribe` / `MemoryChanged` / `MemoryChangeEventDto` roundtrip |
| **unit** | `aibe/src/adapters/outbound/in_process_memory_subscription_broker.rs` | broker publish / kind filter / drop で unregister |
| **integration** | `aibe/tests/memory_subscribe.rs` | apply → publish、専用 socket で `memory_changed` 受信、他 RPC 混在拒否 |

### 0022 output filter の検証観点

正式指示書: [0022_ai-filter-spec.md](done/0022_ai-filter-spec.md)。

| 種別 | ファイル | 担保する観点 |
|------|----------|--------------|
| **unit** | `ai/src/domain/output_filter.rs` | `AI_FILTER` > `[ask].filter` の優先順位 |
| **unit** | `ai/src/adapters/outbound/output_filter.rs` | `/bin/sh -c`、stdin pipe、非ゼロ終了、spawn 失敗 |
| **unit** | `ai/src/adapters/outbound/stdout_presenter.rs` | 空 assistant の stdout 不出力、stderr 非対象 |
| **unit** | `ai/src/adapters/outbound/toml_config.rs` | `[ask].filter` の読み込み |
| **integration** | `ai/tests/ask_integration.rs` | filter 付き `Ask` が完走すること |
| **integration** | `ai/tests/phase_a_cli.rs` | non-TTY で `route_turn` を飛ばし、`AI_SESSION_ID` を request に載せること |
| **integration** | `aibe/tests/route_turn.rs` | route plan の redaction と conversation store の分離 |
| **integration** | `aibe-client/tests/route_turn.rs` | `route_turn` の socket 往復 |
| **manual** | [manual/ai-ask-tools.md](manual/ai-ask-tools.md) | `AI_FILTER` / config filter の stdout 変換、stderr 非対象、失敗時 warning |

### 0048 output filter と streaming の検証観点

設計書: [0048_ai-filter-streaming-fix-spec.md](spec/0048_ai-filter-streaming-fix-spec.md)。

| 種別 | ファイル | 担保する観点 |
|------|----------|--------------|
| **unit** | `ai/src/adapters/outbound/stdout_presenter.rs` | filter 有効時の streaming stdout 抑止、`streamed=true` でも final filter 出力、余計な改行なし |
| **integration** | `ai/tests/phase_a_cli.rs` | streaming 付き `ask` で chunk が stdout に漏れない |
| **manual** | [manual/ai-ask-tools.md](manual/ai-ask-tools.md) | D1b streaming 付き filter |

## モック・フィクスチャ

### 0063 Human Task suspend checkpoint

- `ai/tests/0063_human_task_suspend_checkpoint_red.rs`: checkpoint domain round-trip、実tempdir file storeのmode/symlink/size/atomic replace/invalid保全とcheckpoint欠落残骸、coordinator順序・副作用なしruntime path割当・collision・Done cleanup、root flock保持、statusの一貫読取、Suspended/orphaned Runningの実CLI cancel、cancel後の新規開始、sidecarなしSuspended result保持。
- `aish/tests/0063_human_task_suspend_checkpoint_red.rs`: 生成されたbash/zsh一時rcfile、Rust helperによるversion 1 event送信とUnicode reason validation、最初のterminal event、通常return回帰。
- `aibe/tests/0063_human_task_suspend_checkpoint_red.rs`: scripted LLMの同一roundにhuman_taskと後続toolを置き、実coordinator/file store/statusを通して`SuspendTurn`、LLM 1 call、後続tool 0 call、store再openを検証する。外部providerと実PTYだけをfakeにする。
- Phase 2回帰は0055/0057/0060/0061/0062の既存integration testも直列実行する。

### 0064 Human Task resume

- `ai/tests/0064_human_task_resume_red.rs`: Suspended→resume→再suspendのvertical E2E、cwd/briefing復元、segment追記、root flock保持、複数回suspendのObservation順序、missing/mismatched ID、非Suspended拒否、cwd不在、Done時のResultPending永続化、再suspend最終save失敗時のSuspended復元、Done最終save失敗時のRunning維持、起動後Cancelled/Interrupted/FailedのRunning維持、pre-launch失敗のSuspended復元、初回createの起動後エラーでRunning維持・pre-launchで削除、ParentTermiosGuard配線、0063単一segment/Done削除回帰。

### 0065 Human Task agent continuation

- `ai/tests/0065_human_task_agent_continuation_red.rs`: ResultPending→Continuing→Finished/delete vertical E2E、固定 unverified message、保存済み parent context と current cwd、stable continuation turn ID、状態 invariant、Human Shell 非起動 retry、通常失敗時 ResultPending 復元、root flock、Finished delete fail-closed、status/CLI、Running/Suspended invariant 回帰。
- `aibe/tests/0065_human_task_agent_continuation.rs`: `continuation_turn=true` の `AgentTurnStatus::Ok` ID を同じ RequestService process 内で再送し、2回目を LLM call 前に `invalid_request` で拒否する。通常の provider failure および MaxToolRounds 後は同一 ID 再試行を許可する。
- 0063/0064 の既存 integration test を直列実行し、create Done 削除、Suspended resume、複数 segment、ResultPending 保存を回帰確認する。

### 0066 Human Task recovery hardening

- `ai/tests/0066_human_task_recovery_hardening_red.rs`: orphaned Running→Suspended→既存resumeのvertical E2E、Continuing→ResultPendingのturn ID保持、状態別status案内、corrupt / unsupported相当 / mode不正 / checkpoint欠落残骸の明示force cleanup、confirmation拒否、root flock busy、既存recoverable状態の非上書きを実tempdir storeで検証する。
- `ai` unit tests（`human_task_file_store`）: force cleanup の root path差し替え耐性、nested directory混在時のfail-closed（元residue非改変）、quarantine差し替え後のdecoy保全を検証する。
- PID / lease / heartbeat / reconciler / schema migration / 自動crash recoveryはテストfixtureにも製品契約にも追加しない。bash / zshの新規実PTY確認は`docs/manual/0066_human-task-recovery-hardening.md`に限定する。

- LLM HTTP は **統合/E2E では必ずモック**（wiremock、`httptest`、録画レスポンス等）
- 実 API キーを使うテストを CI に入れない
- フィクスチャは `*/tests/fixtures/` に置き、大きなログは必要最小限

## 手動検証ドキュメント

手順は次の 2 層に分ける。

| 種別 | 置き場所 | 内容 |
|------|----------|------|
| ユーザー向けマニュアル | `docs/manual/<topic>.md` | 使い方・概念・典型フロー |
| 手動検証手順 | `docs/testing/<topic>-manual-verification.md` | 実機確認の前提・手順・期待結果 |

テンプレ（検証手順）:

```markdown
# <機能名> 手動検証

## 前提
- ビルド: `cargo build --workspace`
- 設定: ...

## 手順
1. ...
2. ...

## 期待結果
- ...

## よくある失敗
- ...
```

手動のみの変更を「完了」にする場合、AI は **未実施であること** を報告に明記する。

## カバレッジ

当面は数値カバレッジ目標なし。**境界とプロトコル** を優先してテストを足す。
