# 0025 — CLI サブエージェント実装指示書

> **出典**: [0024_cli-subagent-provider-spec.md](0024_cli-subagent-provider-spec.md)、[manual/cli-subagent-products.md](manual/cli-subagent-products.md)、[0011_llm-profiles-spec.md](done/0011_llm-profiles-spec.md)、[architecture.md](architecture.md)、[testing.md](testing.md)、[security.md](security.md)  
> **状態**: **実装前**  
> **注記**: 本書は 0024 の first-class CLI サブエージェント案に紐づくが、採用しない。代替設計は [0026_external-commands-spec.md](../spec/0026_external-commands-spec.md)。
> **前提**: 本書は 0024 の実装指示書であり、仮実装・サンプル止まりを許可しない。

## 目的

HTTP LLM API に依存せず、**Codex CLI** と **Claude Code CLI** を aish 体験の中でサブエージェントとして使えるようにする。実装は aibe を親エージェント、CLI を子プロセスのサブエージェントとして扱い、CLI 固有のログイン状態・契約・出力形式をそのまま利用する。

この機能は、`aibe-protocol` の wire 変更、`aibe` の CLI ランナー実装、`ai` の `cli-thread.json` 永続化、そして関連 docs 同期を一体で完了させる。

## 対象範囲

### 対象

| 項目 | 実装対象 |
|------|----------|
| 形態 A | サブエージェント専用プロファイルで CLI に直接委譲する経路 |
| 形態 B | HTTP 親プロファイルから `invoke_*` ツール経由で CLI を呼ぶ経路 |
| structured artifacts | `summary_text` / `exit_status` / `changed_files` / `thread_id` |
| 永続化 | `AISH_SESSION_DIR/cli-thread.json` の読み書きと自動 resume |
| 並行性 | トップレベル `max_concurrent_cli` セマフォ |
| 起動検証 | `command` の実行可否チェックと設定エラー化 |
| docs 同期 | `architecture.md` / `testing.md` / `security.md` / manual / spec index |

### 非対象

| 非対象 | 理由 |
|--------|------|
| CLI が aibe の tool_call スキーマで逐次戻すハイブリッド loop | 0024 で明示的に非目標 |
| aish から CLI を直接起動する入口 | レイヤー境界違反 |
| リポジトリ外 cwd への委譲 | 0024 で非対象 |
| 設定ホットリロード | 0011 同様に非対象 |
| `changed_files` の git status フォールバック | 0024 で非対象 |
| Codex MCP と thread 状態の共有 | 別経路、共有しない |
| Windows 対応 | ワークスペースのスコープ外 |
| `--new-cli-thread` のユーザー向け露出 | MVP では必須にしない。必要なら別指示書で扱う |

## 実装方針

### 形態 A

1. `ai ask` が解決した `llm_profile` を `aibe` に送る。
2. `aibe` はその profile が `codex_cli` または `claude_code_cli` の backend に属するか判定する。
3. 属する場合は `ToolRoundExecutor` を回さず、CLI ランナーに直接委譲する。
4. CLI の stdout / structured event から `ClientResponse::AgentTurnResult` と `artifacts` を構成する。
5. `cli-thread.json` があれば profile 一致を確認して resume、なければ初回起動にする。

### 形態 B

1. `aibe` は HTTP 親プロファイルの tool catalog に `invoke_<backend_key>` を公開する。
2. その tool は同じ CLI ランナーを呼び、実行結果を artifacts 付きで返す。
3. 親 LLM 側の tool policy / max rounds / termination strategy は既存の HTTP 経路と同じまま維持する。
4. `invoke_*` の名前は backend table key と 1 対 1 で対応し、`KNOWN_TOOLS` / docs と同期する。

## レイヤー別タスク

### `aibe-protocol`

- `ClientResponse::AgentTurnResult` に `artifacts` を追加する。
- `SubagentArtifacts` 型を新設し、少なくとも次の 4 フィールドを持たせる。
  - `summary_text`
  - `exit_status`
  - `changed_files`
  - `thread_id`
- serde の roundtrip テストを追加し、`artifacts` を含む新しい response 形を固定する。
- 既存の `ClientRequest` の `llm_profile` は維持し、追加の request 破壊変更は入れない。

### `aibe`

- `CliSubagentRunner` を新設し、Codex CLI と Claude Code CLI を共通インターフェースで扱う。
- backend ごとの adapter を分離する。
  - Codex 用: `codex exec` / `codex exec resume`
  - Claude 用: `claude -p ...` / `--resume`
- CLI 出力から artifacts を構築する parser を実装する。
  - Codex: `thread.started` と `file_change` 系イベントを使う
  - Claude: `session_id` と stream-json 編集イベントを使う
- `changed_files` は CLI が出したパス文字列だけを採用し、`git status` などから補完しない。
- Claude で編集ファイルが取れない場合は `changed_files = []` とし、パース不能時だけ error にする。
- CLI が非 0 終了でも、`summary_text` / `thread_id` / `changed_files` を回収できた場合は artifacts として返し、`exit_status` に失敗コードを残す。完全に解析不能な場合だけ error にする。
- `timeout_secs` で子プロセスを kill + reap する。
- `max_concurrent_cli` の semaphore を実装し、待ち時間は CLI 実行 timeout に含めない。
- `command` が PATH で実行できない場合は起動時に `ConfigError` にする。
- CLI 失敗時のログは残しても、秘密情報を新たに出力しない。
- 形態 A と形態 B の分岐を server-side で固定し、HTTP 親経路では既存の tool policy を壊さない。

### `ai`

- `AISH_SESSION_DIR/cli-thread.json` を読み書きする。
- 現在の `llm_profile` と保存済み `cli-thread.json` の `llm_profile` を比較し、一致時だけ resume 可能にする。
- `llm_profile` 不一致、ファイル破損、JSON 解析失敗、または対象外 provider のときは「保存なしの初回起動」とみなす。
- `client_cwd` は従来どおり `std::env::current_dir()` を使い、CLI 側に渡す cwd と一致させる。
- `ai` は CLI 固有の provider ロジックを持たず、`aibe-protocol` と `aibe-client` の外に出ない。
- 形態 B の tool catalog 連携が必要な場合は、`ai` 側の allowlist / tool name 同期を `KNOWN_TOOLS` と合わせて更新する。

### `aish`

- 実装変更なし。
- 既存の session dir / log 連携をそのまま利用する。

## `cli-thread.json` の扱い

### 保存場所

- `AISH_SESSION_DIR/cli-thread.json`
- `aish shell` の session dir 配下に置く
- 別ペインや `--session` の場合でも、同じ session dir を使う

### 保存内容

```json
{
  "llm_profile": "delegate-codex",
  "thread_id": "0199a213-81c0-7800-8aa1-bbab2a035a53",
  "provider": "codex_cli"
}
```

### ルール

- `llm_profile` が現在の request の profile と一致したときだけ resume する。
- `provider` が現在の backend と一致しない場合は resume しない。
- ファイルが存在しない、読めない、壊れている、または一致条件を満たさない場合は初回起動とする。
- 保存は原子的に行い、途中失敗で既存ファイルを壊さない。

## 受け入れ条件

### 1. 形態 A が本番経路で動く

- [ ] CLI backend を選ぶ profile を指定すると、`aibe` は HTTP の ToolRoundExecutor ループを回さず CLI ランナーに直接委譲する。
- [ ] Codex CLI と Claude Code CLI の両方について、初回起動と resume の両方が実装されている。
- [ ] `summary_text` / `exit_status` / `changed_files` / `thread_id` が `ClientResponse::AgentTurnResult.artifacts` に入る。

### 2. 形態 B が本番経路で動く

- [ ] HTTP 親プロファイルから `invoke_*` ツールが見える。
- [ ] `invoke_*` は backend table key ごとに 1 つずつ対応し、`KNOWN_TOOLS` と docs が一致する。
- [ ] 親の tool policy / max rounds / termination strategy は既存の HTTP 経路と同じ動作を保つ。

### 3. `cli-thread.json` の自動 resume が動く

- [ ] 初回実行後に `AISH_SESSION_DIR/cli-thread.json` が作成される。
- [ ] 同じ `llm_profile` で再実行すると、保存済み `thread_id` で resume される。
- [ ] `llm_profile` 不一致時は resume されず、新しい thread/session になる。
- [ ] 破損 JSON / 欠損 / 読み込み失敗では起動失敗にせず、新規起動へフォールバックする。

### 4. `max_concurrent_cli` が有効

- [ ] 既定値は `4` とする。
- [ ] 同時実行数が上限を超えたリクエストは、実行枠が空くまで待機する。
- [ ] 待機時間は個別 CLI の `timeout_secs` に含めない。
- [ ] 実行中の CLI は timeout 到達時に kill + reap される。

### 5. プロトコル変更が揃う

- [ ] `aibe-protocol` の response 形が更新され、`artifacts` を正しく serialize / deserialize できる。
- [ ] 既存の request 側は `llm_profile` を含めて後方互換を維持する。
- [ ] `ai` と `aibe` のテスト fixture が新しい response 形に追従する。

### 6. 起動時検証が揃う

- [ ] 設定された CLI `command` が実行不能なら `aibe` 起動時に失敗する。
- [ ] `claude_code_cli` を設定しているのに `claude` が実行できない環境では、起動が失敗する。
- [ ] CLI backend が 1 つも有効でない通常経路は、従来どおり起動可能である。

### 7. 非対象を壊していない

- [ ] `aish` に CLI 実装を持ち込んでいない。
- [ ] `git status` フォールバックを追加していない。
- [ ] `context.cwd` の基準を aibe プロセス cwd に戻していない。
- [ ] `ai` が LLM HTTP を直接呼ぶ経路を追加していない。

### 8. 検証が通る

- [ ] 単体 / 統合 / フェイク CLI / manual の検証が用意されている。
- [ ] `./scripts/verify.sh` が成功する。

## テスト計画

### 単体テスト

#### `aibe-protocol`

- `ClientResponse::AgentTurnResult` の serde roundtrip
- `SubagentArtifacts` の serde roundtrip
- `artifacts` あり / なしの互換境界

#### `aibe`

- CLI parser が Codex / Claude の event stream から `thread_id` / `changed_files` / `summary_text` を抽出する
- 非 0 終了でも artifacts を返し、`exit_status` に失敗コードを残す
- `cli-thread.json` の読み書きと profile mismatch 判定
- `max_concurrent_cli` の semaphore が超過待ちする
- timeout 到達時に subprocess を kill + reap する
- command 未発見で `ConfigError` になる

#### `ai`

- `cli-thread.json` の read / write ロジック
- 保存済み profile と現在 profile の一致判定
- 壊れた JSON を新規起動にフォールバックする処理
- `AskRequest` に既存の `llm_profile` をそのまま渡すこと

### 統合テスト

#### フェイク CLI

- Codex 用の fake executable を `aibe/tests/fixtures/` に置き、`thread.started` / `file_change` を返す
- Claude 用の fake executable を `aibe/tests/fixtures/` に置き、`session_id` / stream-json を返す
- 1 回目起動と resume の両方を固定する
- 失敗終了、壊れた JSON、途中で event が欠けるケースを固定する
- `invoke_*` 経由でも同じ runner に到達することを固定する

#### `aibe` / `ai` 統合

- `ai ask` から `aibe` へ request を送り、`cli-thread.json` が更新されることを確認する
- 同一 session dir で 2 回目を実行し、resume が効くことを確認する
- profile 不一致時に resume しないことを確認する
- `KNOWN_TOOLS` / tool catalog の同期を確認する

### 手動検証

#### Codex 実機

- `docs/manual/cli-subagent-products.md` の Codex 手順を実機で実行し、`thread_id` と `changed_files` を確認する
- 編集を含む実タスクで resume が効くことを確認する

#### Claude Code 実機

- `docs/manual/cli-subagent-products.md` の Claude 手順を実機で実行し、`session_id` を `thread_id` として扱えることを確認する
- stream-json から編集ファイルが拾えることを確認する

## docs 更新対象

実装と同じ変更で更新する対象を、以下に固定する。

| ドキュメント | 更新理由 |
|-------------|----------|
| `docs/architecture.md` | `ClientResponse.artifacts`、`cli-thread.json`、`max_concurrent_cli`、形態 A/B、レイヤー責務の反映 |
| `docs/testing.md` | unit / integration / fake CLI / manual の検証計画を反映 |
| `docs/security.md` | CLI サブエージェントの秘密情報・ログ・権限・resume 状態の扱いを追記 |
| `docs/manual/cli-subagent-products.md` | 実 CLI の手動検証手順を最新化 |
| `docs/0000_spec-index.md` | 0025 を索引に追加 |
| `docs/done/0011_llm-profiles-spec.md` | CLI backend と `max_concurrent_cli` の位置づけを補足する必要がある場合に更新 |
| `docs/todo/aibe-cli-llm-provider.md` | 既存メモを整理する必要がある場合に更新または完了扱いへ移送 |

## 実装順序の目安

1. `aibe-protocol` で `SubagentArtifacts` と `ClientResponse` 拡張を入れる。
2. `aibe` で CLI runner と parser を作る。
3. `ai` で `cli-thread.json` の読み書きと resume 判定を入れる。
4. 形態 B の `invoke_*` を追加し、tool catalog と同期する。
5. fake CLI テストを先に固定し、その後 manual 手順を更新する。
6. docs を同じ PR で同期する。

## 残リスク

- 既存の CLI 公式出力フォーマットが将来変わると、parser の再固定が必要になる。
- `cli-thread.json` はローカル state なので、ユーザーが別プロファイルへ切り替えたときの再開挙動を誤ると体験劣化が起きる。
- `max_concurrent_cli` の待機が長い場合、ユーザーは「固まった」と感じる可能性がある。必要なら将来 timeout 表示を追加する。
- `Claude Code` は本環境で未実機検証なので、manual の再現は別環境依存になる。
