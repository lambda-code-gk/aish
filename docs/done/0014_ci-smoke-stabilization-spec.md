# 0014 — CI + スモーク指示書 — 仕様ドラフト

> **出典**: ChatGPT レビュー 4 代目の P0 / Sprint 1 第 3 項目。`docs/todo/chatgpt-review-4th-gen/implementation-order.md`、`p0-stabilization.md` §1, §5、`sprints.md` Sprint 1 を前提にした実装前仕様化。`verification.md` で `.github/workflows` と `scripts/smoke-*.sh` の未存在を確認済み。  
> **状態**: **実装済み**（2026-05-26。Codex レビュー反映: smoke の厳密 I/O 検証・索引状態の文言整合）

## 目的

v0.1 安定化のため、PR マージ前の回帰防止を自動化し、mock aibe への導通を smoke で固定する。新機能は足さない。

この指示書は、実装前に以下を固定する。

- GitHub Actions で `cargo fmt --all -- --check` / `cargo clippy --workspace -- -D warnings` / `cargo test --workspace` / `./scripts/check-architecture.sh` を Linux 上で必ず実行する
- `scripts/smoke-mock.sh` で mock aibe + `ai ask` 相当の導通を自動確認する
- CI は実 API を叩かず、`mock` provider のみを使う
- `aish` の「LLM なし」「ネットワークなし」の境界を壊さない

## スコープ

### 対象

- `.github/workflows/ci.yml` の新設
- `scripts/smoke-mock.sh` の新設
- `docs/testing.md` の CI / smoke 方針追記
- `docs/manual/ai-ask-tools.md` の mock aibe 再現手順追記
- `README.md` の Development 節追記
- `docs/0000_spec-index.md` の 0014 行追加（実装前は未着手、マージ時は **実装済み**）

### 対象外

- `LICENSE` の追加
- `provider = "openai"` alias の追加
- P1 の protocol 分離（`aibe-protocol` / `aibe-client`）
- 実 API キーを使う CI / smoke
- CI でのネットワーク依存追加
- `aish` / `aibe` / `ai` の機能追加
- `echo` だけの偽物 smoke、または mock を使わない stub 成功

## 確定した設計判断

| 項目 | 方針 | 理由 |
|------|------|------|
| **Runner** | `ubuntu-latest` のみを使う | このワークスペースは Unix 専用で、CI も実運用の前提に合わせる方が単純で安全 |
| **Workflow 構成** | `.github/workflows/ci.yml` に `verify` と `smoke-mock` の 2 job を置く | lint / test とプロセス導通を分けると失敗原因が追いやすい。`smoke-mock` は `needs: verify` にして PR 回帰防止を明確化する |
| **verify job** | `cargo fmt --all -- --check` / `cargo clippy --workspace -- -D warnings` / `cargo test --workspace` / `./scripts/check-architecture.sh` を順番に実行する | AGENTS.md の DoD と一致させ、境界・品質・回帰を一括で検査する |
| **smoke script 名** | `scripts/smoke-mock.sh` | 既存の manual 文書と名前を揃えやすく、CI と手動再現の対応が読み取りやすい |
| **smoke の内容** | 一時ディレクトリに `AIBE_CONFIG` / `AIBE_SOCKET_PATH` / `AI_CONFIG` を切り、`[llm] provider = "mock"` の aibe を foreground で起動してから `ai ask` を 1 回実行する | 実際の binary と socket を使い、mock だけで end-to-end の導通を確認できる |
| **smoke の確認点** | `stdout` は非空行 1 行のみ `[mock] received: ping`。`stderr` は非空行 1 行のみ `ai: tools enabled: none`（`warning:` 行なし） | manual B1 と同契約。部分一致 `grep` では余計な行を見逃すため、行数と全文を厳密比較する |
| **終了コード** | スクリプトは成功時 `0`。期待値不一致や起動失敗は非 0。`timeout` による打ち切りは GNU `timeout` の `124` を許容する | CI では失敗を明確に返し、ハングだけ別扱いできる |
| **タイムアウト** | smoke の `ai ask` ラウンドトリップに `timeout 180s`、workflow job 全体に `timeout-minutes: 15` | 冷たい CI 環境でもビルド込みで詰まりにくくする一方、無限待ちは避ける |
| **ネットワーク** | CI / smoke では実 API を叩かない。`mock` provider 以外を使わない | aish の境界と秘密情報の扱いを壊さないため |
| **実行位置** | smoke は別 job で実行する | verify と smoke を分けると、lint/test の失敗と導通失敗を切り分けやすい |

> **脚注**: smoke を verify job に同居させる案もあるが、今回は採らない。build キャッシュを 1 job に寄せる利点はあるものの、socket 起動と CLI 導通の失敗を分離した方がレビューと再実行が簡単で、P0 の「回帰防止」を優先しやすい。

## 受け入れ条件

### 1. CI が Linux で品質ゲートを実行する

- `.github/workflows/ci.yml` が追加される
- `ubuntu-latest` 上で `cargo fmt --all -- --check` が通る
- `cargo clippy --workspace -- -D warnings` が通る
- `cargo test --workspace` が通る
- `./scripts/check-architecture.sh` が通る

### 2. mock aibe smoke が自動化される

- `scripts/smoke-mock.sh` が追加される
- smoke は `echo` ではなく、`aibe` と `ai` の実 binary を使って導通する
- smoke は `AIBE_CONFIG` / `AIBE_SOCKET_PATH` / `AI_CONFIG` を一時領域に切り、ユーザー環境の本番設定を読まない
- smoke は実 API に接続しない
- smoke は `ai ask` の 1 回の round-trip を検証する

### 3. CLI / socket の契約を確認する

- `aibe` は mock provider で起動できる
- `ai ask` は `--no-start` と `--socket` を使って mock aibe へ接続できる
- `stdout` は非空行が `[mock] received: ping` の 1 行のみ
- `stderr` は非空行が `ai: tools enabled: none` の 1 行のみ（`warning:` プレフィックスなし）

### 4. docs が同期される

- `docs/testing.md` に CI job と smoke の役割分担が記載される
- `docs/manual/ai-ask-tools.md` に smoke の再現に必要な mock aibe 手順が反映される
- `README.md` の Development 節が CI 実行コマンドと smoke の位置づけに一致する
- `docs/0000_spec-index.md` に 0014 行が追加され、実装完了後は状態が **実装済み** になる

## 実装タスク分解

1. `.github/workflows/ci.yml` を新設し、`verify` と `smoke-mock` の 2 job を定義する
2. `scripts/smoke-mock.sh` を新設し、mock aibe の起動・待機・`ai ask` 実行・期待値検証・後始末を実装する
3. `docs/testing.md` に CI と smoke の役割分担を追記する
4. `docs/manual/ai-ask-tools.md` に smoke 再現用の mock aibe 設定を追記する
5. `README.md` の Development 節に、CI と smoke の標準コマンドを追記する
6. `docs/0000_spec-index.md` に 0014 を追加し、実装完了後に「実装済み」へ更新する

## docs 更新一覧

- `docs/testing.md`
  - CI で実行する 4 コマンドと smoke の役割分担を追記する
  - `cargo test --workspace` と smoke の責務の違いを明記する
- `docs/manual/ai-ask-tools.md`
  - mock aibe 再現手順を smoke と同じ前提に揃える
  - `AIBE_CONFIG` / `AIBE_SOCKET_PATH` / `AI_CONFIG` の隔離を明確にする
- `README.md`
  - Development 節に CI の標準コマンドと smoke の位置づけを足す
- `docs/0000_spec-index.md`
  - 0014 を追加し、実装完了後は実装済みとする

## テスト方針

| 手段 | 担当範囲 | 補足 |
|------|----------|------|
| `cargo test --workspace` | Rust の単体 / 統合 / E2E 相当の自動テスト | `ai/tests/ask_integration.rs` の mock aibe 統合を含め、ロジックと API 契約を守る |
| `scripts/smoke-mock.sh` | 実 binary / 実 socket / config 参照 / プロセス起動順 | `ai ask` の CLI 契約と、mock aibe との end-to-end 導通を固定する |
| GitHub Actions `verify` job | フォーマット・警告・回帰・境界 | PR マージ前の品質ゲート |
| GitHub Actions `smoke-mock` job | 実行パスの最終確認 | networkless で本番経路に近い最小確認 |

### 役割分担の考え方

- `cargo test --workspace` は、ロジック・設定パース・モック通信・回帰を広く網羅する
- smoke は、テストでは拾いにくい「実 binary 起動」「設定ファイル参照」「socket 接続」「`stdout` / `stderr` の CLI 契約」を確認する
- smoke は `cargo test` の代わりではなく、`cargo test` の抜けを埋める最小の導通検査として扱う

## リスク・未確定

| 種別 | 内容 |
|------|------|
| **推測** | `timeout 180s` は冷たい CI 環境でも十分なはずだが、初回の GitHub Actions 実行結果を見て調整が必要になる可能性がある |
| **推測** | smoke を別 job に分けると build が重複しうる。もし CI 時間が厳しければ、将来は cache 強化か job 構成の見直しが必要になる |
| **未確定** | `ai ask` の stderr 文言は既存契約に合わせるが、将来の文言変更に追随する必要がある |
| **未確定** | smoke の期待 stdout は mock の固定応答に依存するため、mock 実装の変更があれば本 spec の文言も更新が要る |

## DoD

この指示書は **仕様ドラフト** であり、ここでは完了判定をしない。実装フェーズで「完了」と報告するには、AGENTS.md の DoD に従い、少なくとも次を満たすこと。

1. 本番経路の実装が入っていること
2. 必要な単体 / 統合 / E2E / smoke が追加されて成功していること
3. 手動検証が必要なら `docs/manual/` に手順があり、未実施なら報告に明記すること
4. 挙動・プロトコル・設定に触れたなら `docs/` を同じ変更で更新すること
5. `cargo fmt --all -- --check` / `cargo clippy --workspace -- -D warnings` / `cargo test --workspace` / `./scripts/check-architecture.sh` を通すこと

