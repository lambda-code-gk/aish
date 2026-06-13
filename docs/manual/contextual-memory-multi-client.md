# contextual memory — multi-client readiness

将来の VSCode extension / TUI / mobile / browser sidecar など、**複数クライアント**が同じ contextual memory を共有・購読するための接続モデルと v1 の制限を説明する。

設計正本: [spec/0037_aibe-contextual-memory-runtime-v1-spec.md](../spec/0037_aibe-contextual-memory-runtime-v1-spec.md) §9 MemorySubscribe / §10 Capability model / §14 Phase 7。  
手動検証（CLI）: [contextual-memory.md](contextual-memory.md)。

## 前提: local runtime

v1 の aibe は **同一マシン上の local runtime** である。

- Unix domain socket + **stdio JSON** で RPC する（[architecture.md](../architecture.md)）
- **remote network daemon** や **認証 / OAuth / token issue** は v1 非対象
- capability model は **将来の multi-client 拡張のための application boundary** であり、現状の `ai` CLI は既定 `local_full`（全 capability 許可）で従来どおり動作する

セキュリティ上、capability check だけで remote 公開が安全になるわけではない。network exposed daemon を実装する場合は別途認証・認可設計が必要（0037 §10.4 / §19）。

## identity: session と memory space

| ID | 役割 | owner |
|----|------|-------|
| `AI_SESSION_ID` | shell log / conversation / runtime session | 各 turn の provenance |
| `memory_space_id` | contextual memory の正本 ID | memory store（`memory/spaces/<id>/events.jsonl`） |
| `AIBE_CONTEXT_ID` | ユーザー向け context 名（CLI 選択） | **クライアント側のみ** |

重要:

- **`AI_SESSION_ID` は memory owner ではない**。session が変わっても同じ `memory_space_id` なら同じ goal / rule 等が見える。
- **`AIBE_CONTEXT_ID` は client-side selection**。`ai` が RPC 送信前に `memory_space_id` として解決し、`MemoryContext` に載せる。**サーバ `aibe` は環境変数 `AIBE_CONTEXT_ID` を読まない**（複数クライアント接続時にサーバ env で全員の context が変わるのを防ぐ）。

解決順（**クライアント `ai` のみ**）:

1. 環境変数 `AIBE_CONTEXT_ID`
2. `~/.config/ai/config.toml` の `[context] current`（`ai context use`）
3. cwd から導出した `project_<hash>`
4. `legacy_session_<session_id>`（非推奨）

サーバ側フォールバック（クライアントが ID を載せなかった場合）: request 明示 `memory_space_id` > cwd project > legacy session のみ。

## 共有例: 複数 session、同一 memory space

以下は **同一 `memory_space_id`（例: `ctx_team_a`）** を、異なる `AI_SESSION_ID` から参照する例である。

```bash
cargo build -p aibe -p ai
export PATH="$PWD/target/debug:$PATH"
# aibe デーモンまたは mock が起動済みであること

# --- ターミナル A: session sess_alice ---
export AI_SESSION_ID=sess_alice
export AIBE_CONTEXT_ID=ctx_team_a
ai goal set "Contextual Memory Runtime v1 を完成させる"
ai now set "Phase 7 docs を書く"

# --- ターミナル B: session sess_bob（別 session） ---
export AI_SESSION_ID=sess_bob
export AIBE_CONTEXT_ID=ctx_team_a
ai goal show
# → sess_alice と同じ goal が表示される

ai now show
# → now の本文は見えるが、別 session で更新されていないため stale 表示されうる

ai mem add rule "idea は通常クエリへ常時注入しない"
# → 次の agent turn から rule が pinned 注入される（両 session 共通）

# --- ターミナル C: 別 memory space ---
export AI_SESSION_ID=sess_carol
export AIBE_CONTEXT_ID=ctx_team_b
ai goal show
# → ctx_team_a の goal は見えない（space 分離）
```

正本ファイル: `~/.local/share/aibe/memory/spaces/ctx_team_a/events.jsonl`（aibe 側）。`ai` / `aish` は memory をローカル正本として保持しない。

## 将来クライアントの接続モデル

v1 では **aibe-protocol の stdio JSON** が契約である。将来クライアントも同じ RPC を使う想定。

```text
┌─────────────┐     Unix socket      ┌──────────────┐
│  ai (CLI)   │◄────────────────────►│    aibe      │
│  VSCode ext │   1 RPC / 1 行 JSON  │  (local)     │
│  TUI client │                      │              │
│  mobile *   │                      │ memory store │
└─────────────┘                      └──────────────┘
         │                                    ▲
         │  MemoryContext.memory_space_id      │ MemoryApply /
         └──────────────────────────────────────┘ Recipe apply
```

各クライアントが行うこと:

1. 接続確立（Unix socket）
2. **自クライアント側**で `memory_space_id` を解決（`AIBE_CONTEXT_ID` 相当の UI / 設定）
3. `memory_apply` / `memory_query` / `agent_turn` 等の RPC に `MemoryContext { cwd, memory_space_id }` を載せる
4. （任意）変更通知が必要なら **subscribe 専用接続**を別途張る（下記）

v1 非対象（0037 §19）: VSCode extension 本体、mobile クライアント本体、remote P2P、cloud sync、network exposed daemon authentication。

### mobile 向け profile（将来例）

mobile クライアントは **shell execute を持たない** 設計とする（0037 §10.3）。

```text
mobile_memory:
  memory:read
  memory:write
  memory:archive
  memory:recipe_run
  memory:subscribe
  agent:ask
  # shell:propose / shell:execute は含めない
```

v1 では `StaticCapabilityPolicy` に `memory_only` / `memory_read_only` 等の profile が存在するが、**wire 上の capability 交渉は未実装**。将来クライアントは composition root で policy を選ぶ想定。

## Capability 分離

memory 操作権限と shell 実行権限は **AIBE application service boundary** で分離する（0037 Phase 6）。`ai` 側には capability model を持ち込まない。

| Capability | 主な操作 |
|------------|----------|
| `MemoryRead` | `MemoryQuery`、recipe 材料収集 |
| `MemoryWrite` | `MemoryApply(Add)` |
| `MemoryArchive` | `MemoryApply(Archive)` / `ClearKind` |
| `MemoryRecipeRun` | `MemoryRecipeRun`（apply 時は Write/Archive も） |
| `MemorySubscribe` | `MemorySubscribe` |
| `AgentAsk` | `AgentTurn`（LLM 呼び出し） |
| `ShellPropose` | turn 内 `shell_exec` 提案 |
| `ShellExecute` | `shell_exec` 実行（承認 UI とは独立） |

既定 profile:

| profile | 用途 |
|---------|------|
| `local_full` | 現行 `ai` CLI 互換（全 capability） |
| `memory_read_only` | read / subscribe / agent ask のみ（write / archive 拒否） |
| `memory_only` | memory + agent ask（**shell execute 拒否**） |

shell 承認（`shell_exec_approval`、tier、pattern auto-approve）は **ShellExecute capability とは別レイヤー**。memory capability があっても shell は別途判定される。

## MemorySubscribe（v1 制限）

目的: 同一 aibe process 内で、`MemoryApply` / recipe apply 成功時に **購読クライアントへ push** する（0037 §9）。

### 必須（v1 で提供）

- in-process broker が `memory_changed` を publish
- `memory_space_id` でフィルタ（optional `kind` filter あり）
- 接続切断で subscriber 解放

### 非目標（v1 では提供しない）

- ファイルシステム watch
- daemon 再起動後の **reconnect / replay**
- remote network subscription
- exactly-once delivery

### Transport: subscribe 専用接続

`AgentTurn` の streaming / approval と同型。

1. クライアントは **subscribe 専用**の Unix socket 接続を張る
2. `MemorySubscribe` RPC を 1 行 JSON で送る
3. サーバは `MemorySubscribeResult` を 1 行 JSON で返す
4. 以降、同一接続へ `MemoryChanged` を **push** する
5. **同一接続で `MemoryApply` / `AgentTurn` 等の他 RPC は混在不可**

```json
{"type":"memory_subscribe","id":"…","session_id":"sub_001","context":{"cwd":null,"memory_space_id":"ctx_team_a"},"kind":"goal"}
```

初期応答:

```json
{"type":"memory_subscribe_result","id":"…","status":"ok","memory_space_id":"ctx_team_a"}
```

通知例:

```json
{"type":"memory_changed","id":"…","memory_space_id":"ctx_team_a","event":{"kind":"goal","change":"added","entries":[…]}}
```

CLI `ai` は v1 では subscribe UI を持たない。TUI / IDE クライアントが専用接続で購読する想定。

## CLI recipe apply semantics（`ai mem run clarify-goal --apply`）

`ai mem run clarify-goal --apply` は **2 段階**で store を更新する（0037 §8.5）。

1. `MemoryRecipeRun(apply=false)` で LLM 提案（`summary` / `proposals[]`）を取得する
2. CLI が対話確認（`Apply proposed memory operations? [y/N]`）後、各 `proposal.operation` を **`MemoryApply` として個別送信**する

この経路はサーバ側 `MemoryRecipeRun(apply=true)` とは異なる。

| 経路 | store 更新 | subscription `change` |
|------|-----------|------------------------|
| `MemoryRecipeRun(apply=true)` | サーバが一括 apply | まとめて `recipe_applied` |
| CLI `--apply`（proposal → 個別 `MemoryApply`） | クライアントが複数回 apply | 各 operation に応じて `added` / `status_changed` 等（`recipe_applied` ではない） |

仕様上問題ない。subscribe クライアントは **最終的な memory state** を見ればよく、apply 経路の差はイベント種別の違いにとどまる。CLI `--apply` は非対話 stdin では fail-closed（確認不能のため apply しない）。

## 関連ドキュメント

- [contextual-memory.md](contextual-memory.md) — CLI 手動検証
- [architecture.md](../architecture.md) — プロトコル・identity・RPC 一覧
- [security.md](../security.md) — memory と shell のセキュリティ境界
