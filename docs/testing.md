# テスト方針

aish ワークスペースのテスト種別、置き場所、実行方法。機能変更時は **テストとこの文書を同じコミットで更新** する。

## 実行コマンド（標準）

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
./scripts/check-architecture.sh   # クレート境界 + scripts/check-hexagonal.sh（レイヤー）
```

ローカルでも PR 前に上記を通す。GitHub Actions（[`.github/workflows/ci.yml`](../.github/workflows/ci.yml)）でも同じ品質ゲートを回す。

### CI と smoke の役割分担

| 手段 | 内容 |
|------|------|
| **`verify` job** | `fmt` / `clippy` / `cargo test --workspace` / `check-architecture.sh` |
| **`smoke-mock` job** | [`scripts/smoke-mock.sh`](../scripts/smoke-mock.sh) — 実 binary・実 socket・`provider = "mock"` で `ai ask` 1 回 |
| **ローカル smoke** | `./scripts/smoke-mock.sh`（CI と同じ。実 API キー不要） |

`cargo test --workspace` はロジック・プロトコル・モック統合（例: `ai/tests/ask_integration.rs`）を広く網羅する。smoke は **CLI の `stdout` / `stderr` 契約** と設定ファイル参照・プロセス起動順を、テストでは拾いにくい経路で固定する。smoke は `cargo test` の代替ではない。

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
