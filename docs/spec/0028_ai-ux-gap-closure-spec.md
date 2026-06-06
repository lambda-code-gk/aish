# 0028 — `ai` UX ギャップ解消 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-06  
> **関連**: [0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)、[turn_cancel.rs](../../ai/src/application/turn_cancel.rs)、[main.rs](../../ai/src/main.rs)、[agent_turn.rs](../../aibe/src/application/agent_turn.rs)

## 目的

`0027` で `ai` の日常利用 UX の土台は入ったが、実運用で重要なギャップが残っている。本書はそのうち優先度の高いものだけを閉じる。

対象は次の 6 点である。

1. `ai chat` の client-side transcript
2. exit code の意味分け
3. preset の `shell_exec_approval` を実行時に反映する優先順位
4. provider からの真の assistant streaming
5. `--dry-run` の filter マスク
6. `chat` の `Ctrl+C` handler 二重登録の扱いの確定

本書は新しい大きな UX を増やすのではなく、`0027` の未解決部分を正しい境界で閉じることを目的とする。

## 0027 との関係

`0027` は `ai` のコマンド面・履歴・preset・`chat` REPL・`--dry-run` の方向を定義した。本書はそのうち、実装が未完了または部分実装のまま残っている部分を補完する。

- `0027` で「client-side transcript」とした方針を、実際の `agent_turn.messages` 構築と `history` 記録まで落とす
- `0027` で定義した exit code 仕様を、現在の `main.rs` のエラー分類に合わせて確定する
- `0027` の preset 定義にあった `shell_exec_approval` を、実行時の優先順位まで含めて固める
- `0027` が要求した assistant streaming を、provider 実装にまで落とす
- `0027` の `--dry-run` マスクを、filter コマンドの秘匿まで含めて厳格化する

`chat` の `Ctrl+C` handler 二重登録については、現状の `ai/src/application/turn_cancel.rs` が `Once` で 1 回だけ登録する実装になっており、テストもある。**0028 では新規に別 handler を増やさず、既存実装を参照するだけ**とする。

## 非スコープ

- history payload の GC
- smoke テストへの `history` / `chat` 全面追加
- `aibe` に first-class conversation 永続化を導入すること
- `aish` の session layout 変更
- provider 追加

## 現状認識

現行実装には次の状態がある。

- `ai/src/main.rs` の `run_chat` は各入力を独立 turn として送っている
- `ai/src/main.rs` の `execute_turn` は local history に `conversation_id: None` を記録している
- `ai/src/main.rs` の exit code は `anyhow` の成否に依存しており、`0/2/3/4/5/130` に分解されていない
- `AiPresetConfig` には `shell_exec_approval` があるが、実行時の優先順位は `yes_exec` 周辺に閉じていない
- `OpenAiCompatibleLlm` / `GeminiLlm` / `MockLlm` は現状 buffered final 応答であり、provider 起点の streaming はない
- `--dry-run` の report は今後の拡張で raw 値混入を起こし得るため、秘匿ルールを明文化する必要がある

## 機能仕様

### 1. `ai chat` の client-side transcript

`ai chat` は multi-turn REPL だが、会話状態は `aibe` に持たせない。`ai` クライアントが transcript を保持し、各 turn の `agent_turn.messages` に過去の user/assistant を積み上げる。

#### 仕様

- `chat` 起動時に、空の transcript を 1 つ生成する
- transcript は client process 内で保持する
- transcript に入れるのは model-visible な user/assistant の履歴である
- 1 turn ごとに、前回までの transcript 全体を `agent_turn.messages` として送る
- turn が `AgentTurnResult` で完了したら、送信した user message と final assistant message を transcript に append する
- `Error` / `Cancelled` の場合は transcript を更新しない
- tool 呼び出しの中間 message は `aibe` の内部ループに閉じ、client-side transcript には保存しない
- `conversation_id` は `chat` session ごとに 1 つ生成し、同じ REPL 内の全 turn で使い回す
- `history` には各 turn の `conversation_id` を記録する

#### `retry` / `rerun` との関係

- `retry` / `rerun` は従来どおり `history_id` ベースで動く
- `chat` 由来の履歴に `conversation_id` がある場合は、それをそのまま残す
- `ask` など単発 turn では `conversation_id` は省略可とする

### 2. exit code 意味分け

`ai` の終了コードは次の意味に固定する。

| exit code | 意味 |
|-----------|------|
| `0` | 正常終了。`AgentTurnStatus::MaxToolRounds` もここに含める |
| `2` | usage error / local validation / `invalid_request` |
| `3` | transport / connect / decode / timeout / 非 SIGINT の cancel |
| `4` | remote `provider_error` |
| `5` | remote `tool_error` / `tool_timeout` / `tool_not_allowed` |
| `130` | `Ctrl+C` / SIGINT |

#### 仕様

- `0` は「結果が返った」ことを意味し、`MaxToolRounds` は warning 扱いのまま `0` を維持する
- `2` は CLI 引数不正、設定不正、history の再生不可能、`aibe` からの `invalid_request` を含む
- `3` は socket 接続失敗、NDJSON decode 失敗、transport エラー、`--timeout` 到達時のキャンセル完了を含む
- `4` は provider 起因の失敗に対応する
- `5` は tool 実行の失敗に対応する
- `130` はユーザーの SIGINT に対応する。`Ctrl+C` によって cancel が完了した場合も、トップレベルコマンドの終了理由が SIGINT なら 130 を返す
- `3` と `130` が競合する場合は `130` を優先する

### 3. preset の `shell_exec_approval` と `--yes-exec`

`shell_exec_approval` は aibe 側の設定値だが、`ai` は preset を読み込み、実行時に反映する。

#### 優先順位

1. CLI の明示指定
2. preset
3. aibe config

#### 現行 CLI での解釈

- `--yes-exec` は現行 CLI 側の explicit opt-in として扱う
- `--yes-exec` は `shell_exec_approval=ask` の場合にだけ prompt bypass を許す
- `shell_exec_approval=never` は `--yes-exec` より強い
- `shell_exec_approval=always` は prompt を出さないので `--yes-exec` は実質無効

#### 仕様

- `resolve_turn_settings` は config と preset をマージした後、`--yes-exec` を評価する
- `--yes-exec` による session 限定の記憶は既存 `YesExecCache` を使う
- cache のスコープは `AISH_SESSION_DIR` 単位、無い場合は current process 単位とする
- `shell_exec_approval` の最終決定値は audit できる形で history / log に残す

### 4. provider からの真の assistant streaming

OpenAI-compatible / Gemini / mock の provider は、assistant 本文を「最後にまとめて返す」だけでなく、可能なものは streaming delta を emit する。

#### 仕様

- `aibe` は provider 実装から assistant delta を受け取れる path を持つ
- streaming 対応 provider は、生成中に `assistant_streaming` event を順次 emit する
- streaming 非対応 provider は、final text を 1 回だけ synthetic delta として emit する
- synthetic delta は 1 turn あたり 1 回に限定する
- final `AgentTurnResult` の本文は従来どおり最終的な assistant message を運ぶ
- client 側は streaming event を表示し、final response で確定する

#### provider 別

- OpenAI-compatible: backend が stream 対応なら真の streaming を使う
- Gemini: backend が stream 対応なら真の streaming を使う
- mock: テスト・開発用として真の streaming を模擬できるが、最低限は final text から synthetic delta 1 回でよい

#### 収束条件

- provider ごとの差は「stream を本当に受け取るか」だけに閉じる
- client/UI に見える最終意味は同じにする

### 5. `--dry-run` の filter マスク

`--dry-run` の report には、filter コマンドの中身を出してはいけない。

#### 仕様

- raw message は出さない
- raw shell log tail は出さない
- `filter` / `ask_filter` / `output_filter` の生文字列は出さない
- replay payload の raw data は出さない
- 代わりに、source、長さ、`preset`、解決済みの構造、`filter` の有無だけを出す

#### 表示方針

- `filter` は `enabled=true` / `source=preset|config|env` / `masked=true` のようなメタ情報に変換する
- `DiagnosticsReport` / `DryRunReport` に raw の `ask_filter` や resolved `output_filter` を載せない
- `output_filter` の実コマンド文字列を stdout / stderr / JSON / TSV / ENV のどこにも含めない
- `DiagnosticsReport` と `DryRunReport` の両方で同じ秘匿規則を適用する

### 6. `chat` の `Ctrl+C` handler 二重登録

この項目は、**現状の実装が正であるかを確認し、正しいなら参照のみ**とする。

#### 仕様

- `turn_cancel` の handler は process 全体で 1 回だけ登録する
- `chat` の REPL ループや `execute_turn` から、新しい SIGINT handler を重ねて登録しない
- もし既存実装がこの条件を満たしているなら、0028 では新規コードを追加しない

## クレート配置

| クレート | 責務 |
|---------|------|
| `ai` | `chat` transcript、`conversation_id`、history 記録、exit code の決定、`shell_exec_approval` の最終解決、`--dry-run` マスク |
| `aibe` | provider streaming の event 化、assistant delta の emit、final response への収束 |
| `aibe-protocol` | 既存 `AssistantStreaming` / `CancelTurn` / `ErrorCode` を正本として維持。必要になったときだけ最小限拡張 |
| `aish` | 変更不要 |

`ai` は `aibe` 本体へ path 依存を増やさず、既存の `aibe-client` / `aibe-protocol` を通す。

## セキュリティ

### 1. transcript は client-side に閉じる

- `chat` transcript は `ai` process 内に閉じる
- `aibe` に永続 conversation state を持たせない
- local history には `conversation_id` だけを記録し、全文 transcript を無制限に複製しない

### 2. dry-run は秘匿優先

- filter コマンドの生文字列を表示しない
- shell log tail を表示しない
- replay payload の raw data を表示しない
- マスクは TSV / JSON / ENV の全形式に適用する

### 3. shell_exec 承認の危険性

- `--yes-exec` は dangerous tool の自動承認につながる
- preset で `shell_exec_approval` を有効化する場合は、利用者が危険性を理解している前提とする
- `shell_exec_approval=never` は明示的な拒否として尊重する

### 4. SIGINT の意味を曖昧にしない

- `Ctrl+C` はユーザーの中断として 130 に対応させる
- 通信失敗や timeout と混同しない
- `quiet` は診断の量を減らすだけで、失敗の意味を隠さない

## 受け入れ条件

### unit

- `chat` の transcript が turn ごとに蓄積される
- `conversation_id` が local history に記録される
- exit code が `0/2/3/4/5/130` に分岐する
- preset / `--yes-exec` / aibe config の優先順位が固定される
- streaming 非対応 provider が synthetic delta 1 回を emit する
- `--dry-run` の report が filter の生文字列を含まない
- `turn_cancel` の handler 二重登録が起きない

### integration

- `ai chat` で複数 turn の transcript が同じ `conversation_id` で history に残る
- `ai ask --dry-run` が raw filter / raw log tail を出さない
- `--yes-exec` が `shell_exec_approval=ask` のときだけ効く
- streaming 対応 provider では delta が逐次表示される
- streaming 非対応 provider でも 1 回の synthetic delta 後に final response が出る

### manual

- `Ctrl+C` で 130 が返る
- `ai chat` で turn を跨いだ会話が維持される
- `ai ask --dry-run` で filter の中身が見えない

## 実装 Phase

### Phase 1: client-side transcript と exit code

- `ai chat` の transcript を追加する
- `conversation_id` を生成し、history に記録する
- exit code を `0/2/3/4/5/130` に分解する

### Phase 2: preset と dry-run の秘匿

- preset の `shell_exec_approval` を実行時解決に入れる
- `--yes-exec` の最終優先順位を固める
- `--dry-run` の report から raw filter を除去する

### Phase 3: provider streaming

- OpenAI-compatible / Gemini / mock に stream-capable path を入れる
- 非対応時の synthetic delta 1 回を標準化する
- `assistant_streaming` と final response の順序を固定する

### Phase 4: 既存 Ctrl+C 実装の参照確認

- `turn_cancel` の `Once` ベース実装がそのまま維持されているか確認する
- 問題がなければ新規 handler は追加しない

## 未確定事項

本書では次を未確定のまま残さない。

- `chat` transcript の保持単位は process 内固定
- `conversation_id` は `chat` session ごとに 1 つ
- `--yes-exec` は `ask` モードのみを bypass する
- streaming 非対応時の fallback は synthetic delta 1 回
