# 0038 — Contextual Memory Pack Phase B（TurnHook / RpcExtension trait 化）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（Phase B 実装済み）  
> **起票**: 2026-06-15  
> **関連**: [0038_contextual-memory-pack-phase-a-spec.md](0038_contextual-memory-pack-phase-a-spec.md)、[0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)、[manual/contextual-memory.md](../manual/contextual-memory.md)

## 0. 目的

Phase A で導入した `[memory] enabled` による basic 切替を、`aibe` 内部の **pack 境界** に昇格させる。

Phase B の目的は次の 2 点である。

1. `agent_turn` の memory 注入を `TurnHook` trait に分離する
2. `memory_apply` / `memory_query` / `memory_kind_list` / `memory_recipe_run` / `memory_subscribe` の RPC 分岐を `RpcExtension` trait に分離する

これにより、同一バイナリのまま `memory.enabled` に応じて pack を差し替えられるようにし、将来の memory クレート分離や動的ロードの前提を整える。

### 0.1 Phase A / Phase C との関係

- Phase A は **runtime toggle** である。`enabled=false` のときは今の basic 契約を維持する
- Phase B は **trait 化と pack 合成** である。同一バイナリのまま internal boundary を正す
- Phase C 以降で、CLI / built-in kind / 将来の memory クレート分離を進める

Phase B では **動的プラグインロードは行わない**。pack はあくまで in-process の合成単位である。

## 1. 非目標

- memory クレートの分離
- 動的プラグインロード
- `ai` 側の新しい memory policy 導入
- wire protocol / DTO の再設計
- `[memory] enabled` の廃止
- Phase C 以降の CLI / builtin kind の pack 移行

## 2. 現状と課題

### 2.1 Phase A の実装状況

現状の Phase A は次のように実装されている。

- `aibe/src/application/server.rs`
  - `memory_config.enabled` で `FilesystemContextualMemoryStore` か `EmptyContextualMemoryStore` を選ぶ
- `aibe/src/application/request_service.rs`
  - `RequestService.memory_enabled` で `memory_apply` / `memory_query` / `memory_kind_list` / `memory_recipe_run` / `memory_subscribe` をガードする
- `aibe/src/application/agent_turn.rs`
  - `AgentTurnService.memory_enabled` で `prepare_turn_messages` の memory 注入をスキップする
- `aibe/src/application/memory_runtime.rs`
  - 無効時の固定エラーメッセージを提供する
- `ai/src/application/memory_cli.rs`
  - `ai goal` / `now` / `idea` / `mem` / `context` の CLI 側ガードを持つ

### 2.2 なぜ trait 化が必要か

Phase A の `memory_enabled` は機能として正しいが、責務が 2 つの service に散っている。

- prompt 注入の分岐が `AgentTurnService` にある
- RPC 分岐が `RequestService` にある
- その結果、memory の有効/無効は「単一の pack を選ぶ」というより「複数 service に散った bool 判定」になっている

この構造のままだと次の問題がある。

- 追加の pack を導入したいときに service ごとの分岐が増える
- memory 以外の機能 pack を同じ方法で差し替える基盤にならない
- basic / contextual の両実装が service 内部に混在しやすい
- 将来の memory クレート分離時に、依存の切り出しが難しい

Phase B はこの bool 判定を、**pack を組み立てる責務** に移す。

## 3. 設計概要

### 3.1 Pack の考え方

pack は「同じ機能群をまとめて差し替える in-process の単位」である。

memory pack については、少なくとも次の 2 つの構成を持つ。

- `BasicPack`
  - memory 注入なし
  - memory RPC 拒否
- `ContextualMemoryPack`
  - contextual memory 注入あり
  - memory RPC を実装

`server.rs` の composition root は `memory.enabled` を見て pack を選ぶだけにする。

### 3.2 依存方向

Phase B で目指す依存方向は次のとおり。

- `server.rs` が pack を組み立てる
- `RequestService` と `AgentTurnService` は pack の trait を consume する
- contextual memory の具体実装は `adapters` に残す
- pack trait の定義は `aibe` 内の `ports` か、同等の境界モジュールに置く
- `memory.enabled` を参照するのは composition root の `server.rs` のみとする。`RequestService` / `AgentTurnService` は pack object だけを見る

Phase B では `ai` クレートの責務は変えない。`ai` は引き続き CLI フロントエンドであり、memory policy は持たない。

## 4. TurnHook trait

### 4.1 役割

`TurnHook` は `AgentTurn` の prompt 組み立てに割り込むための trait である。

Phase B では、現在の `prepare_turn_messages` 相当の処理をここへ切り出す。

### 4.2 呼び出しタイミング

呼び出しタイミングは現状と同じである。

1. system instruction を前置する
2. shell log tail を前置する
3. `TurnHook` で追加の user-context を注入する
4. その後にユーザーの実 query を送る

つまり `TurnHook` は **`prepare_turn_messages` の memory 部分** を担当し、routing には関与しない。

### 4.3 シグネチャ方針

`Send + Sync` な同期 trait とする。prompt 注入は turn 前処理の一部なので、この層では async 化しない。

```rust
pub trait TurnHook: Send + Sync {
    fn prepare_turn_messages(
        &self,
        context: &AgentTurnContext,
        messages: Vec<ChatMessage>,
    ) -> Result<Vec<ChatMessage>, TurnHookError>;
}
```

この設計の意図は次のとおり。

- `AgentTurnContext` から必要情報を取れるようにする
- `messages` の順序制御は hook 側に閉じ込める
- hook が no-op の場合はそのまま `messages` を返せる
- hook 失敗時は turn 全体を失敗させず、呼び出し側で元の `messages` を使う。diagnostic は内部ログに留める

`TurnHookError` は hook 専用の失敗を表す。Phase B の contextual memory 実装では、prompt 注入失敗は **best-effort** として扱い、原則として turn 全体を失敗させない。

### 4.4 basic 実装

`BasicPack` の `TurnHook` は no-op である。

- 入力 messages をそのまま返す
- memory store / resolver を触らない
- `enabled=false` の契約を維持する

### 4.5 contextual memory 実装の責務分割

contextual memory 実装の `TurnHook` は次を担当する。

- `memory_space_id` の解決
- prompt block の取得
- `MEMORY_PROMPT_BUDGET_BYTES` に沿った注入
- `goal` / `now` / `idea` / `rule` 等の既存 resolver 挙動の利用

逆に担当しないものは次のとおり。

- RPC の受け口
- memory 永続化
- subscribe push
- `ai` 側の CLI policy

## 5. RpcExtension trait

### 5.1 役割

`RpcExtension` は memory 系 RPC をひとまとめに扱う trait である。

Phase B で拡張対象にする RPC は次の 5 つで固定する。

- `memory_apply`
- `memory_query`
- `memory_kind_list`
- `memory_recipe_run`
- `memory_subscribe`

### 5.2 ディスパッチ方式

ディスパッチは `RequestService` 内の `match ClientRequest` を保ちながら、memory 系だけを `RpcExtension` に委譲する。

`RpcExtension` は `async_trait` もしくは boxed future で object safety を保つ。

重要なのは次の点である。

- wire 上の request/response 形式は変えない
- `RequestService` は inbound handler のまま
- pack は request の中身を解釈するが、wire schema には触れない
- `memory_subscribe` の接続維持と push loop は transport 層が担当し、`RpcExtension` は購読開始に必要な結果だけ返す

`memory_subscribe` は専用接続が必要なため、`RpcExtension` は通常 RPC の結果だけでなく subscribe 開始処理も提供する。

```rust
#[async_trait]
pub trait RpcExtension: Send + Sync {
    fn memory_apply(...) -> ClientResponse;
    fn memory_query(...) -> ClientResponse;
    fn memory_kind_list(...) -> ClientResponse;
    async fn memory_recipe_run(...) -> ClientResponse;
    fn memory_subscribe_begin(...) -> (ClientResponse, Option<MemorySubscription>);
}
```

### 5.3 basic 実装

`BasicPack` の `RpcExtension` は拒否実装とする。

- 返すのは Phase A と同じ固定メッセージ
- `InvalidRequest` として fail-closed にする
- `memory_subscribe` も同様に初回応答で拒否する

ここでの「未登録」は将来の pack registry 拡張で使える概念にとどめ、Phase B の basic 実装は **拒否** を正とする。これにより Phase A の契約を壊さない。

### 5.4 contextual memory 実装の責務分割

contextual memory 実装の `RpcExtension` は、既存の service 群を内部で束ねる役割を持つ。

- `memory_apply` / `memory_query` / `memory_kind_list` は `MemoryService`
- `memory_recipe_run` は `MemoryRecipeService`
- `memory_subscribe` は `MemorySubscribeService`
- `memory_subscribe` の初回応答後の `memory_changed` push は transport 側に残す。extension は subscription の発行だけを担う

このとき、extension は「どの service を呼ぶか」を隠蔽し、composition root からは 1 つの pack として扱えるようにする。

## 6. Composition root

### 6.1 server.rs の責務

`aibe/src/application/server.rs` は pack の composition root とする。

`memory.enabled` を見て次を選ぶ。

- `enabled=false`
  - `BasicPack`
  - `EmptyContextualMemoryStore` と静的 built-in loader を使い、`<AIBE_ROOT>/memory/*.toml` の parse/merge は行わない
- `enabled=true`
  - `ContextualMemoryPack`
  - filesystem store / registry loader を組み立てる

`server.rs` は hook / extension / store / resolver / broker を組み立てて `RequestService` と `AgentTurnService` に渡す。

### 6.2 期待する構成

Phase B では、少なくとも次の shape を目標にする。

- `server.rs`
  - pack 選択
  - 共有依存の生成
- `RequestService`
  - `RpcExtension` を通じて memory 系 RPC を処理
- `AgentTurnService`
  - `TurnHook` を通じて prompt 注入を処理

この構成により、`memory_enabled` は service 内の散在した bool ではなく、composition root の pack 選択に集約される。

## 7. クレート境界

### 7.1 aibe 内の配置方針

Phase B での配置方針は次のとおり。

- `aibe/src/ports/`
  - `TurnHook` / `RpcExtension` の trait 定義
  - pack 用の最小 DTO / context
- `aibe/src/application/`
  - pack の組み立て
  - `RequestService` / `AgentTurnService` からの利用
- `aibe/src/adapters/outbound/`
  - filesystem store
  - memory space resolver
  - subscription broker
  - `BasicPack` / `ContextualMemoryPack` の具体実装

この配置により、trait と実装の依存方向が明確になる。

### 7.2 ai 側の変更範囲

Phase B では **ai のソースコード変更は非目標** とする。

- `ai/src/application/memory_cli.rs` の既存ガードは維持する
- `ai` は引き続き `memory.enabled` の設定に従う
- `ai` が memory policy を持つようにはしない

つまり、Phase B は aibe 内の pack 化で閉じる。

## 8. 後方互換・設定

### 8.1 設定互換

`[memory] enabled` は Phase A と同じ意味を維持する。

- `enabled=true`
  - contextual memory pack を使う
- `enabled=false`
  - basic pack を使う

環境変数オーバーライドの優先順位も Phase A のまま維持する。

### 8.2 破壊しないこと

Phase B では次を壊さない。

- `enabled=false` の固定エラーメッセージ
- memory 注入なし
- memory RPC 拒否
- `ai` CLI の起動前ガード

このため、`BasicPack` は「機能を隠す」のではなく、既存の拒否契約をそのまま再現する。

## 9. 受け入れ条件

- [x] `server.rs` が pack を組み立てる
- [x] `TurnHook` が `prepare_turn_messages` 相当の責務を担う
- [x] `RpcExtension` が memory 系 5 RPC を束ねる
- [x] `enabled=false` で `BasicPack` が選ばれ、現行の拒否契約が維持される
- [x] `enabled=true` で contextual memory pack が選ばれる
- [x] `memory_enabled` の bool 判定が service 内に散らばらない
- [x] wire schema と `aibe-protocol` は変更しない
- [x] `ai` の挙動は Phase A から変えない
- [x] `./scripts/verify.sh` が成功する

## 10. テスト方針

Phase B では、既存の `memory_disabled` 系テストを **移行または維持** しながら、pack boundary のテストを追加する。

### 10.1 既存テストの扱い

- `aibe/tests/memory_disabled.rs`
- `ai/tests/memory_disabled_cli.rs`

これらは Phase B 後も、`enabled=false` の契約を守れていることを確認する回帰テストとして残す。

### 10.2 追加したいテスト

- `BasicPack` の `TurnHook` が no-op であること
- `BasicPack` の `RpcExtension` が固定メッセージで拒否すること
- `ContextualMemoryPack` が既存 service 群を正しく束ねること
- `server.rs` の pack 選択が `memory.enabled` に一致すること

### 10.3 テストの焦点

Phase B のテストでは、実装の細部よりも次を重視する。

- pack 選択が single source of truth になっていること
- basic / contextual の契約差分が明示されていること
- subscribe の専用接続契約が維持されていること

## 11. リスク・未確定事項

### 11.1 未確定

- `TurnHook` / `RpcExtension` の厳密なファイル配置
- `RpcExtension` の subscribe 初期応答をどこまで trait に含めるか
- pack 用の context DTO を既存 service context と共用するか、専用型にするか

### 11.2 リスク

- trait を細かく切りすぎると、かえって service 間の橋渡しが増える
- basic pack の拒否実装が Phase A の固定メッセージからずれると、CLI / tests が壊れる
- pack 化を急ぎすぎると、将来の memory クレート分離より先に内部抽象が肥大化する

> 推測: Phase B の最適解は「pack は 1 つの selected object として扱い、複数 pack の同時合成は Phase C 以降に回す」形である。現時点では single-pack selection の方が検証しやすく、Phase A の契約も保ちやすい。
