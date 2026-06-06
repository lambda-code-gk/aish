# 0026 — 外部コマンド（CLI コーディングエージェント）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 実装済み  
> **起票**: 2026-06-06  
> **実装指示**: [0026_external-commands-implementation-spec.md](../done/0026_external-commands-implementation-spec.md)  
> **関連**: [architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[todo/aibe-cli-llm-provider.md](../todo/aibe-cli-llm-provider.md)、[codex-delegation.md](../codex-delegation.md)、[0024_cli-subagent-provider-spec.md](../done/0024_cli-subagent-provider-spec.md)、[0025_cli-subagent-implementation-spec.md](../done/0025_cli-subagent-implementation-spec.md)、[manual/cli-subagent-products.md](../manual/cli-subagent-products.md)

## 目的

Codex CLI や Claude Code CLI を使いたいが、`feature/cli-subagent` のような **aibe の first-class サブエージェント統合** は採用しない。代わりに、既存の `shell_exec` 経路に乗る **外部コマンド** として明示的に登録し、AISH のポリシー外であることを分かる形にする。

本書は、CLI coding agent を「LLM プロバイダ」や「ツール実装」に昇格させず、**既存の shell 実行ゲートを使った設定テンプレート**として扱うための設計境界を固定する。

## 背景

`feature/cli-subagent` では、Codex CLI / Claude Code CLI を aibe の親子ループに first-class 統合する案が実装された。だが、AISH の既存方針は次の通りで、そこに新しい LLM ループや thread 共有を持ち込むと境界が崩れる。

- safe tools と dangerous tool の境界は `docs/architecture.md` と `docs/security.md` が正本
- `shell_exec` は `@exec` または literal 指定でのみ許可
- `ai` は LLM を直接呼ばず、`aibe-client` だけを使う
- aibe の責務はツール実行と監査であって、外部 CLI の内部状態管理ではない

そのため 0024 / 0025 の「CLI サブエージェント統合」は採用せず、**外部コマンド** として再定義する。

## 非目標

- 新しい `LlmProvider` 種別の追加
- `invoke_*` のような first-class ツールの追加
- `ClientResponse.artifacts` / `ClientRequest.cli_resume` などの protocol 拡張
- CLI thread の自動 resume
- `aibe` 常駐からの MCP 呼び出し
- `feature/cli-subagent` の本番マージ
- CLI の内部 sandbox / login / tool catalog を aibe が代理管理すること

## 0024 / 0025 との関係

0024 / 0025 は「Codex CLI / Claude Code CLI を aibe に first-class 統合する」案だったが、本書では **非採用** とする。`docs/done/0024_cli-subagent-provider-spec.md` と `docs/done/0025_cli-subagent-implementation-spec.md` は、実装の正本ではなく **歴史的な比較資料** として残す。

本設計（0026）は、0024 / 0025 の代替である。差分は単純で、aibe が CLI を新しいプロバイダやツールとして扱うのではなく、`shell_exec` で起動する外部コマンドに戻す。

## 信頼境界

### AISH ポリシー内

- `@read-only` / `@exec` / `@full` のカテゴリ判定
- `shell_exec` の allowlist
- 実行前承認
- timeout / kill / reap
- `tool_calls` 監査
- `cwd` の解決と `context.cwd` の強制

### AISH ポリシー外

- Codex CLI / Claude Code CLI のログイン状態
- CLI 自身の内部ツール
- CLI 自身の sandbox やネットワーク許可
- CLI の thread / session の永続化
- CLI の出力フォーマット変更

外部コマンドは AISH が「安全化した」と主張しない。AISH が保証するのは、**既存の shell 実行ゲートを通したこと**だけである。

## 最小経路

外部 CLI を呼ぶために新しい制御面は不要で、既存の次の経路で足りる。

1. `ai` が `@exec` か literal 指定を有効化する
2. `aibe` が `shell_exec` を allowlist で検証する
3. `aibe` が承認・timeout・監査を行う
4. `shell_exec` が外部 CLI を実行する
5. CLI の stdout をユーザー向け応答として扱う

この経路だけで足りるため、次のものは導入しない。

- `LlmProvider` 拡張
- `invoke_*` ツール
- `artifacts`
- `cli-thread.json`
- 自動 resume
- `max_concurrent_cli` の専用セマフォ

## 軽量プリセット

外部コマンドは、aibe の新機能ではなく **設定テンプレート**として定義する。TOML 上の案は `[[external_commands]]` とする。

```toml
[[external_commands]]
name = "codex"
description = "Codex CLI を shell_exec で呼ぶ外部コマンド"
command = "codex"
args = ["exec", "--json", "-C", "{cwd}", "{prompt}"]
timeout_secs = 1800

[[external_commands]]
name = "claude"
description = "Claude Code CLI を shell_exec で呼ぶ外部コマンド"
command = "claude"
args = ["-p", "{prompt}", "--output-format", "json"]
timeout_secs = 1800
```

### スキーマ案

- `name`: プリセット名。ツール名ではない。UI や docs で参照するための識別子
- `description`: 人間向け説明
- `command`: 実行するバイナリ名またはラッパースクリプト名
- `args`: argv テンプレート。shell string ではなく、必ず配列として扱う
- `timeout_secs`: そのプリセットで使う上限時間

### 整合ルール

- `command` は `tools.shell_exec.allowed_commands` に含める
- ラッパースクリプトを使うなら、そのラッパースクリプト名を allowlist に入れる
- ここでいうプリセットは allowlist を置き換えない。**allowlist の上に乗る注釈**にすぎない

### 明示しないこと

- first-class tool 化しない
- `provider = "codex_cli"` / `provider = "claude_code_cli"` のような LLM provider 種別は置かない
- `thread_id` / `session_id` を AISH 側で保存しない
- 自動 resume をしない

## フロー

```mermaid
flowchart LR
  U[ユーザー入力] --> A[ai ask]
  A --> B[aibe\n既存 tool loop]
  B --> C{LLM が\n外部コマンドを選ぶ}
  C -->|@exec / literal shell_exec| D[shell_exec\nallowlist + approval]
  D --> E[Codex / Claude CLI]
  E -->|stdout/stderr| D
  D -->|tool result / audit| B
  B --> A
```

ここでの「外部コマンド」は、あくまで `shell_exec` の一回実行である。`invoke_*` のような中間ツールはない。CLI が返した stdout 以外の構造化情報は、AISH の契約では扱わない。

## Cursor Codex MCP との役割分担

`docs/codex-delegation.md` の Codex MCP は **Cursor 親 → Codex MCP サブエージェント** 用であり、aibe 常駐とは別経路である。thread を共有しない。

本書の外部コマンドは、MCP ではなく **shell_exec の外部プロセス** である。つまり、次の 2 つは混同しない。

- Cursor の Codex MCP: 親子の会話スレッドを持つ
- AISH の外部コマンド: 一回の shell 実行として扱う

## セキュリティ・監査

- `risk_class` は `DangerousShell` のまま扱う
- `approval_state` は既存の `shell_exec_approval` に従う
- `approval_source` は少なくとも `shell_exec_approval=<mode>` を追跡し、必要ならプリセット名も識別できる文字列にする
- `ai` は外部コマンドの存在を warning として明示する
- ユーザーは CLI のログイン状態・ネットワーク・内部ツール権限を自分で管理する

### ユーザー責任の明示

外部コマンドは AISH が安全性を保証する対象ではない。特に、CLI が行う編集・削除・ネットワーク送信・外部 API 呼び出しは、AISH の safe tools ポリシーの外にある。AISH は shell 実行の gate だけを提供し、実際の操作責任はユーザーにある。

## `ai` の allowlist / warning / カテゴリ

- 外部コマンドは `@exec` または literal `shell_exec` の範囲でのみ使う
- `@full` は `shell_exec` を暗黙には有効化しない
- `ai` は起動時に、外部コマンドの利用可能性を `shell_exec` 警告として表示する
- `ai` のカテゴリ追加はしない。`@exec` のままで足りる

## `feature/cli-subagent` から捨てるもの

- `codex_cli` / `claude_code_cli` を `LlmProvider` にする案
- `invoke_*` の first-class ツール
- `ClientResponse.artifacts`
- `ClientRequest.cli_resume`
- CLI thread の保存と自動 resume
- `max_concurrent_cli` の専用共有セマフォ
- `changed_files` などの structured artifacts 前提
- CLI を aibe の親子 loop に載せる設計

## `feature/cli-subagent` から残せるもの

- Codex / Claude のコマンドライン調査結果
- 実行 timeout や output 形式の観察メモ
- fake CLI を使った契約テストの発想
- 手動検証で使うコマンド例

ただし、これらは **外部コマンドのテンプレート設計に使う参考資料**にとどめる。first-class 統合の根拠にはしない。

## 実装フェーズへの引き渡し

受け入れ条件・タスク分解・テスト計画は [0026 実装指示書](../done/0026_external-commands-implementation-spec.md) に書いた。

実装フェーズで満たすべき要件（概要）:

1. `[[external_commands]]` が設定として読める
2. `command` が `allowed_commands` と整合していなければ起動時に弾ける
3. 外部コマンドは既存の `shell_exec` 経路だけで起動できる
4. `tool_calls` の監査に `risk_class` / `approval_state` / `approval_source` が残る
5. `@full` が外部コマンドを暗黙有効化しない
6. 0024 / 0025 の first-class 統合を導入していない
7. `./scripts/verify.sh` が通る

## 手動検証の概要

提案ファイル名: `docs/manual/external-commands-cli.md`

最低限の確認項目:

1. `@exec` で外部コマンドが有効になっていることを確認する
2. `ai` の起動 warning に外部コマンドの存在が出ることを確認する
3. Codex / Claude の外部コマンドが allowlist を通って実行されることを確認する
4. timeout / deny / approval の見え方を確認する
5. CLI の内部 thread を AISH が保存しないことを確認する

## 未確定事項

- `args` のプレースホルダ名を `{prompt}` / `{cwd}` で固定するか
- stdin で渡すか argv で渡すか
- `approval_source` にプリセット名を含めるか
- stdout が raw text か JSON かのどちらを主に扱うか
- wrapper script を正式に許容するか
- `ai` の warning 文言をどこまで詳細にするか

## 関連

- [0000_spec-index.md](../0000_spec-index.md)
- [aibe.config.example.toml](../aibe.config.example.toml)
- [architecture.md](../architecture.md)
- [security.md](../security.md)
- [testing.md](../testing.md)
