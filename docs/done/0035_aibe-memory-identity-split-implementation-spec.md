# 0035 — AIBE Memory Identity Split 実装指示書

> **設計書**: [spec/0035_aibe-memory-identity-split-spec.md](../spec/0035_aibe-memory-identity-split-spec.md)  
> **状態**: 実装指示書  
> **注意**: この文書は実装の手順と受け入れ条件を定義する。コード実装は行わない。

## 実装の前提

- 正本は [0035 設計書](../spec/0035_aibe-memory-identity-split-spec.md)
- 実装順は `protocol → domain → store → aibe handler → ai CLI → tests → docs`
- `AI_SESSION_ID` は provenance、`memory_space_id` は ownership という分離を崩さない
- 既存の 0034 由来データは破壊しない

## 変更ファイル一覧

### `aibe-protocol`

- `aibe-protocol/src/memory.rs`
- `aibe-protocol/src/request.rs`
- `aibe-protocol/src/response.rs`
- `aibe-protocol/src/lib.rs`

### `aibe`

- `aibe/src/domain/contextual_memory.rs`
- `aibe/src/ports/outbound/contextual_memory_store.rs`
- `aibe/src/adapters/outbound/contextual_memory_store.rs`
- `aibe/src/application/memory_service.rs`
- `aibe/src/application/request_service.rs`
- `aibe/src/application/agent_turn.rs`
- `aibe/src/application/protocol_convert.rs`
- `aibe/src/adapters/inbound/unix_socket_server.rs`
- `aibe/tests/contextual_memory.rs`

### `ai`

- `ai/src/clap_cli.rs`
- `ai/src/application/memory_cli.rs`
- `ai/src/adapters/outbound/aibe_client.rs`
- `ai/src/adapters/outbound/toml_config.rs`
- `ai/src/main.rs`
- `ai/tests/phase_a_cli.rs`
- 必要に応じて `ai/tests/context_cli.rs` を追加

### `docs`

- `docs/0000_spec-index.md`
- `docs/architecture.md`
- `docs/testing.md`
- `docs/security.md`
- `docs/manual/README.md`
- `docs/manual/contextual-memory.md`
- 必要に応じて `docs/manual/ai-smart-entry.md`

## 実装手順

### 1. protocol

#### 対象

- `aibe-protocol/src/memory.rs`
- `aibe-protocol/src/request.rs`
- `aibe-protocol/src/response.rs`
- `aibe-protocol/src/lib.rs`

#### 作業内容

- `MemoryContext` に `memory_space_id: Option<String>` を追加する
- `MemoryEntryDto` を `memory_space_id` / `created_session_id` / `last_session_id` を含む形へ拡張する
- `ClientRequest::MemoryApply` / `ClientRequest::MemoryQuery` の serde 契約を更新する
- `ClientResponse::MemoryApplyResult` / `MemoryQueryResult` が新しい DTO を返せるようにする
- `deny_unknown_fields` と既存の unknown field reject を維持する
- 既存テストを更新し、`round-trip` と `unknown field reject` を固定する

#### 受け入れ条件

- `MemoryContext` が `cwd` と `memory_space_id` を表現できる
- `MemoryEntryDto` が ownership と provenance を分離して表現できる
- 旧クライアント相当の `memory_space_id = null` でも wire が壊れない
- unknown field は引き続き reject される

### 2. domain

#### 対象

- `aibe/src/domain/contextual_memory.rs`
- 必要に応じて `aibe/src/application/protocol_convert.rs`

#### 作業内容

- `MemorySpaceId` と `MemoryResolution` と `MemoryFreshness` 相当の型を導入する
- `ProjectKey` から canonical な `memory_space_id` を導出する責務を domain / resolver 側へ明示する
- 解決順を固定する
  1. request の明示 `memory_space_id`
  2. `AIBE_CONTEXT_ID`
  3. `cwd` から導出した project-backed id
  4. `legacy_session_<session_id>`
- `now` の stale 判定を `last_session_id != current_session_id` ベースで扱えるようにする
- `MemoryEntry` の owner を `memory_space_id` に移し、`session_id` は provenance に降格させる
- `MemoryEntry::to_dto` で新しい DTO に変換できるようにする

#### 受け入れ条件

- 同じ `project_key` から同じ canonical `memory_space_id` が得られる
- `AIBE_CONTEXT_ID` が project-backed id より優先される
- legacy fallback が `session_id` 単位で成立する
- `now` の stale 状態を domain で表現できる

### 3. store

#### 対象

- `aibe/src/ports/outbound/contextual_memory_store.rs`
- `aibe/src/adapters/outbound/contextual_memory_store.rs`

#### 作業内容

- 保存先を `~/.local/share/aibe/memory/spaces/<memory_space_id>/events.jsonl` に移す
- lock の粒度を session から memory space に変更する
- 既存の legacy layout `~/.local/share/aibe/conversations/<AI_SESSION_ID>/memory/events.jsonl` を read-through で参照できるようにする
- named context の初回アクセス時に legacy data を new layout へ lazy copy する
- 既存の 0700 / 0600 の権限を維持する
- `ctx_a` と `ctx_b` の分離と共有の両方が成立するよう、query/apply の解決を memory space 単位で行う

#### 受け入れ条件

- 同じ `memory_space_id` を別 `AI_SESSION_ID` から参照して同じ state を見られる
- `ctx_a` を共有する `sess_001` と `sess_002` が同じ memory を読む
- `ctx_b` を持つ `sess_003` が `ctx_a` の goal を読まない
- legacy fallback でも request が落ちない
- legacy data を new layout へ copy しても元の session store を壊さない

### 4. aibe handler

#### 対象

- `aibe/src/application/memory_service.rs`
- `aibe/src/application/request_service.rs`
- `aibe/src/application/agent_turn.rs`
- `aibe/src/application/protocol_convert.rs`
- `aibe/src/adapters/inbound/unix_socket_server.rs`

#### 作業内容

- `MemoryService` が `memory_space_id` を含む `MemoryContext` を受け取り、resolution に渡す
- `agent_turn` の prompt 注入が `memory_space_id` 単位で動くようにする
- `now` が stale の場合に表示・prompt 注入でそれが分かるようにする
- `MemoryApplyResult` / `MemoryQueryResult` で DTO の新フィールドを返す
- `request_service` / inbound handler で memory 系 request の新スキーマを通す

#### 受け入れ条件

- prompt block が memory space ownership に従う
- `now` の stale 表現が turn へ反映される
- 旧 request と新 request の両方で handler が動く

### 5. ai CLI

#### 対象

- `ai/src/clap_cli.rs`
- `ai/src/application/memory_cli.rs`
- `ai/src/adapters/outbound/aibe_client.rs`
- `ai/src/adapters/outbound/toml_config.rs`
- `ai/src/main.rs`
- `ai/tests/phase_a_cli.rs`
- 必要に応じて `ai/tests/context_cli.rs`

#### 作業内容

- `ai context current`
- `ai context use <NAME>`
- `ai context new <NAME>`
  を追加する
- `~/.config/ai/config.toml` に current context を保存できるようにする
- memory 系 RPC 送信時に `memory_space_id` を `MemoryContext` に載せる
- 送信する `cwd` は絶対パスを維持する
- `ai context current` と `ai mem show` の表示で `memory_space_id` と provenance を分ける
- `goal / now / idea / mem` の既存コマンドが新しい context 解決に従うようにする

#### 受け入れ条件

- `ai context current/use/new` が最小 MVP として動く
- `ai goal set/show/clear` が同じ context を参照する
- `ai mem show` が current context を明示できる
- `AIBE_CONTEXT_ID` が local config より優先される

### 6. tests

#### 対象

- `aibe/tests/contextual_memory.rs`
- `ai/tests/phase_a_cli.rs`
- 必要に応じて `ai/tests/context_cli.rs`
- 各クレートの `#[cfg(test)]` / serde round-trip

#### 作業内容

- protocol の round-trip と unknown field reject を更新する
- domain の resolve / stale / fallback を単体テスト化する
- store の `sess_001` / `sess_002` / `sess_003` と `ctx_a` / `ctx_b` の共有・分離を固定する
- ai CLI の context command と memory 参照を統合テスト化する
- legacy fallback と lazy copy の安全性を固定する

#### 受け入れ条件

- 仕様に書かれた共有・分離・fallback のテストが自動化される
- `./scripts/verify.sh` で落ちない
- `./scripts/smoke-mock.sh` でも CLI の流れが壊れない

### 7. docs

#### 対象

- `docs/architecture.md`
- `docs/testing.md`
- `docs/security.md`
- `docs/manual/README.md`
- `docs/manual/contextual-memory.md`
- 必要に応じて `docs/manual/ai-smart-entry.md`
- `docs/0000_spec-index.md`

#### 作業内容

- architecture.md の memory 節を identity split 後の正本に更新する
- testing.md のテスト方針へ memory identity split の検証観点を追加する
- security.md に ownership / stale / leakage / lazy copy の扱いを追加する
- manual/contextual-memory.md に context コマンドと sess/context マトリクスを追加する
- manual/README.md に新しい手動検証ページのリンクを足す
- 0000_spec-index.md の tasks セクションに本指示書を追加する

#### 受け入れ条件

- 実装と docs が同じ変更で同期される
- 仕様・指示書インデックスから tasks が辿れる
- 手動検証手順が残る

## テストケース詳細

### protocol

- `MemoryContext` に `memory_space_id` を含めて serialize / deserialize できる
- `MemoryEntryDto` に `memory_space_id` / `created_session_id` / `last_session_id` が含まれる
- unknown field を引き続き reject する
- 旧クライアント相当の `memory_space_id = null` を受けても wire が壊れない

### domain

- `cwd` から project-backed `memory_space_id` を導出できる
- `AIBE_CONTEXT_ID` が project-backed id より優先される
- `legacy_session_<session_id>` fallback が働く
- `now` が別 session で stale と判定される
- legacy data を named context に lazy copy した後、別 session から同じ `memory_space_id` を参照できる

### store

- `memory/spaces/<memory_space_id>/events.jsonl` に保存される
- 同じ `memory_space_id` を別 `AI_SESSION_ID` から参照して同じ goal を見られる
- `ctx_a` を共有する `sess_001` と `sess_002` が同じ memory を見る
- `ctx_b` を持つ `sess_003` が `ctx_a` の goal を見ない
- legacy fallback で `memory_space_id` 未指定 request が落ちない
- legacy data が new layout に lazy copy された後も元の session store を壊さない

### ai CLI

- `ai context current` が current resolution を表示する
- `ai context use <NAME>` が local config を更新する
- `ai context new <NAME>` が新しい current context を作る
- `ai goal set` / `show` / `clear` が `memory_space_id` を通して同じ context を参照する
- `ai mem show` が `memory_space_id` と provenance を分けて表示する

## シナリオ詳細

### sess_001 / ctx_a

- `sess_001` で `ctx_a` を current context にする
- `ai goal set "ship memory split"` を実行する
- `ai now set "stabilize prompt injection"` を実行する
- `ai goal show` と `ai now show` で同じ memory space を参照できることを確認する
- 期待結果: `ctx_a` の正本が作成され、goal / now が `memory_space_id = ctx_a` に保存される

### sess_002 / ctx_a

- `sess_002` で同じ `ctx_a` を current context にする
- `ai goal show` を実行する
- `ai mem show` を実行する
- 期待結果: `sess_001` で作った goal が見える
- 期待結果: `now` は同じ文脈として見えるが、stale 表示または stale メタデータを伴う

### sess_003 / ctx_b

- `sess_003` で `ctx_b` を current context にする
- `ai goal show` と `ai mem show` を実行する
- 期待結果: `ctx_a` の goal / now は見えない
- 期待結果: `ctx_b` は `ctx_a` と独立した memory space として初期化される

### legacy fallback

- `memory_space_id` を明示しない request を送る
- 期待結果: `legacy_session_<session_id>` へフォールバックし、request が失敗しない

## docs 更新リスト

- `docs/architecture.md`
  - contextual memory 節を `memory_space_id` 正本へ更新する
  - `MemoryContext` / `MemoryEntry` の意味を identity split 後に合わせる
  - `ai` の context 解決順を明記する
- `docs/testing.md`
  - 新しい protocol / domain / store / CLI の検証観点を追加する
  - `sess_001/002/003` と `ctx_a/ctx_b` のケースをテスト一覧に載せる
  - `./scripts/verify.sh` と `./scripts/smoke-mock.sh` の役割を崩さない
- `docs/security.md`
  - memory ownership を session ではなく space に置く
  - stale now と leakage の扱いを更新する
  - legacy copy と path-safe id の前提を追記する
- `docs/manual/contextual-memory.md`
  - `ai context current/use/new` を含める
  - sess/context の共有・分離手順を追加する
  - stale now の見え方を明記する
- `docs/manual/README.md`
  - `contextual-memory.md` へのリンクを追加する
- `docs/0000_spec-index.md`
  - `tasks` セクションに本指示書を追加する

## verify / smoke 手順

### 標準検証

```bash
./scripts/verify.sh
```

### スモーク

```bash
./scripts/smoke-mock.sh
```

### 重点確認

- `cargo test -p aibe --test contextual_memory`
- `cargo test -p ai --test phase_a_cli`
- `cargo test -p aibe-protocol`

### 判定基準

- `verify` が通る
- `smoke-mock` が通る
- 重点確認で protocol / store / CLI のいずれも落ちない

## 受け入れ条件

1. `AI_SESSION_ID` が memory の owner ではない
2. `memory_space_id` が contextual memory の owner である
3. `AIBE_CONTEXT_ID` で同じ memory space を複数 session から共有できる
4. `cwd` から project-backed memory space を導出できる
5. `memory_space_id` 未指定時は legacy session fallback がある
6. `MemoryContext` と `MemoryEntry` が split 後の意味を表現できる
7. `ai context current/use/new` が最小 MVP として動く
8. `now` は work/current context として stale を扱える
9. goal / idea / decision / rule の正本は session ではなく memory space に属する
10. 既存の 0034 で保存されたデータを破壊しない

