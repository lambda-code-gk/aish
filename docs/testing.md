# テスト方針

aish ワークスペースのテスト種別、置き場所、実行方法。機能変更時は **テストとこの文書を同じコミットで更新** する。

## 実行コマンド（標準）

```bash
./scripts/verify.sh
```

個別に回す場合:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo build -p aibe   # aibe-client 統合テストが spawn するバイナリ
cargo test --workspace --exclude aibe-client
cargo test -p aibe-client -- --test-threads=1
./scripts/check-architecture.sh   # クレート境界 + subprocess 方針 + check-hexagonal.sh
./scripts/check-docs-consistency.sh   # README / 仕様索引 / testing.md 表の実在パス
```

ローカルでも PR 前に `verify.sh` を通す。GitHub Actions（[`.github/workflows/ci.yml`](../.github/workflows/ci.yml)）の `verify` job も同じ。

### CI と smoke の役割分担

| 手段 | 内容 |
|------|------|
| **`verify` job** | `./scripts/verify.sh`（`aibe-client` は直列テスト。`| tail` で包むと無出力に見えるので避ける） |
| **`smoke-mock` job** | [`scripts/smoke-mock.sh`](../scripts/smoke-mock.sh) — 実 binary・実 socket・`provider = "mock"` で `ai ask` 1 回 |
| **ローカル smoke** | `./scripts/smoke-mock.sh`（CI と同じ。実 API キー不要） |

`cargo test --workspace` はロジック・プロトコル・モック統合（例: `ai/tests/ask_integration.rs`、`aibe/tests/ai_ask_e2e.rs`）を広く網羅する。smoke は **CLI の `stdout` / `stderr` 契約** と設定ファイル参照・プロセス起動順を、テストでは拾いにくい経路で固定する。smoke は `cargo test` の代替ではない。

### 0017 以降のクレート別テスト配置

| クレート | 単体 | 統合 / E2E |
|----------|------|------------|
| **aibe-protocol** | `ClientRequest` / `ClientResponse` / `ToolName` の serde（crate 内 `#[cfg(test)]`） | — |
| **aibe-client** | socket 往復の契約固定（`agent_turn` 承認 prompt → approval → final、TTY 非依存） | `transport.rs`、`tests/agent_turn_approval.rs`、`client_ping.rs`、`ensure_running_*.rs` |
| **ai** | 承認 UI: 非対話 stdin fail-closed、制御文字 escape 表示 | `adapters/outbound/shell_exec_approval_ui.rs`（`#[cfg(test)]`） |
| **aish** | セッション prune 順序・CLI 引数 | `session_store.rs`、`tests/session_cli.rs` |
| **ai** | `--session` hex 検証・presenter / allowlist / output filter | `shell_log_resolve.rs`、`output_filter.rs`、`stdout_presenter.rs`、`ask_integration.rs` |
| **aibe** | server / agent / tools / 承認 | `socket_protocol.rs`、`agent_turn_loop.rs`、`shell_exec.rs`、`shell_exec_approval_socket.rs` |

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
- **E2E**: デーモン起動 → クライアント 1 リクエスト → 応答（ネットワーク不要な fixture 推奨）
- **手動**: 実プロバイダ + 実キーでの 1 ターン（`openai_compatible` / Gemini — `docs/manual/aibe-openai-compatible.md` 等）

### aish

- **単体**: ログ行フォーマット、イベント組み立て
- **統合**: 固定コマンド（`echo`, `false`）の実行とログファイル内容
- **手動**: 対話シェルで入出力がログに残ること（`docs/manual/aish-shell-log.md`）

### ai

- **単体**: 設定読み込み、ログ tail 抽出
- **統合**: モック aibe サーバへ接続して表示
- **E2E**: モック aibe + フィクスチャログで 1 セッション
- **手動**: `ai` → 実 aibe → 表示（キーはユーザー環境のみ）。ツール allowlist は `docs/manual/ai-ask-tools.md`

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
- **0023**: `ai` は `shell_exec_approval_ui` の unit で TTY/escape を固定。`aibe-client/tests/agent_turn_approval.rs` で transport 往復を固定。pipe への `printf y` は [manual/ai-ask-tools.md](manual/ai-ask-tools.md) B3c で手動確認。
- **正本**: 検証計画の説明はこの文書に置き、運用手順は `docs/manual/ai-ask-tools.md` に寄せる。

### 0022 output filter の検証観点

正式指示書: [0022_ai-filter-spec.md](done/0022_ai-filter-spec.md)。

| 種別 | ファイル | 担保する観点 |
|------|----------|--------------|
| **unit** | `ai/src/domain/output_filter.rs` | `AI_FILTER` > `[ask].filter` の優先順位 |
| **unit** | `ai/src/adapters/outbound/output_filter.rs` | `/bin/sh -c`、stdin pipe、非ゼロ終了、spawn 失敗 |
| **unit** | `ai/src/adapters/outbound/stdout_presenter.rs` | 空 assistant の stdout 不出力、stderr 非対象 |
| **unit** | `ai/src/adapters/outbound/toml_config.rs` | `[ask].filter` の読み込み |
| **integration** | `ai/tests/ask_integration.rs` | filter 付き `Ask` が完走すること |
| **manual** | [manual/ai-ask-tools.md](manual/ai-ask-tools.md) | `AI_FILTER` / config filter の stdout 変換、stderr 非対象、失敗時 warning |

## モック・フィクスチャ

- LLM HTTP は **統合/E2E では必ずモック**（wiremock、`httptest`、録画レスポンス等）
- 実 API キーを使うテストを CI に入れない
- フィクスチャは `*/tests/fixtures/` に置き、大きなログは必要最小限

## 手動検証ドキュメント

手順は `docs/manual/<topic>.md` に書く。テンプレ:

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
