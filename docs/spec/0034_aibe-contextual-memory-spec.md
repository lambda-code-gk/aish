# 0034 — AIBE Contextual Memory MVP 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-11  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[0035_aibe-memory-identity-split-spec.md](0035_aibe-memory-identity-split-spec.md)、[0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)、[0019_aish-session-log-integration-spec.md](../done/0019_aish-session-log-integration-spec.md)、[0005_request-context-domain-spec.md](../done/0005_request-context-domain-spec.md)、[0017_aibe-protocol-client-split-spec.md](../done/0017_aibe-protocol-client-split-spec.md)

## 目的

`aibe` に **memory_space_id 単位の contextual memory store** を追加し、`ai` から `goal` / `now` / `idea` を管理できるようにする。

MVP の狙いは次の 4 点である。

1. `goal` と `now` を通常の問い合わせに自動注入する
2. `idea` は on-demand のみ注入する
3. 正本を `aibe` に置き、`aish` は変更しない
4. memory を system instruction にせず、**user-maintained context block** として扱う

本書は、`0030` の smart entry を壊さずに、会話状態とは別軸で「今の目的・状況・着想」を保持するための正規ルートを定義する。

## 非目標

- `aish` のログ形式や export ルールを変更すること
- `ai` から LLM を直接呼ぶこと
- memory を system instruction に昇格させること
- `goal` / `now` / `idea` 以外の高機能ノートシステムを MVP で導入すること
- 全プロジェクト横断のグローバル memory を MVP で導入すること
- Windows 対応

## 背景

### 現状

`aibe` は `AI_SESSION_ID` ごとの conversation store を持ち、`route_turn` と `agent_turn` を分けている。会話の正本は conversation store、入口の正本は `ai` 側の smart entry である。

しかし現在は、`ai` が query 文と aish ログを渡すだけで、**ユーザーが明示的に維持した文脈** を aibe 側で保存・再注入する経路がない。

その結果、次のような情報は会話履歴にも aish ログにも埋もれやすい。

- 直近の作業目標
- いま何を進めているか
- 採用しなかったが残しておきたいアイデア

### 0030 との関係

0030 では `route_turn` が query と recent summary をもとに route plan を返し、`ai` がそれを advisory として扱う。

本書ではこの責務分離を維持する。

- `route_turn` は **生の query** と recent summary のまま使う
- memory の自動注入は **`AgentTurn` でのみ** 行う
- したがって memory は routing の入力を汚染しない

この分離により、route 判定の軽量さと、prompt 側の文脈注入を両立する。

## 用語

### contextual memory

ユーザーが手動で管理する、LLM に見せるための文脈ブロック。system instruction ではなく、ユーザーが後から編集できる prompt 前提情報として扱う。

### memory_space_id

contextual memory の **owner**。0035 で `AI_SESSION_ID` から分離された正本 ID。詳細は [0035_aibe-memory-identity-split-spec.md](0035_aibe-memory-identity-split-spec.md) を参照。

### AI_SESSION_ID

runtime session の provenance。shell log、conversation、request の継続に使う。**memory の owner ではない**。

### project_key

`cwd` から決まるプロジェクト識別子。MVP では次の規則で決める。

1. `cwd` から `.git` root を求める
2. 取れた場合はその **canonicalize 後の絶対パス** を `project_key` にする
3. `.git` root が取れない場合は `cwd` 自体を canonicalize した絶対パスを `project_key` にする

この値は path traversal のための入力ではなく、**同一プロジェクトの同一視** のための正規化キーである。
`MemoryApply` / `MemoryQuery` ではクライアントが `project_key` を送らず、`cwd` から aibe 側で authoritative に再導出する。`project_key` が wire に含まれていた場合は `invalid_request` とする。
project scope の entry は `project_key` で論理分離される。物理保存は `memory_space_id` 配下の JSONL であり、owner は `memory_space_id` である（0035）。

### MemoryEntry

汎用 memory モデル。MVP では少なくとも次の属性を持つ。

- `kind`: 文字列。`goal` / `now` / `idea` は予約済みの standard kind
- `scope`: session と project を区別できる汎用スコープ
- `inject`: `pinned` / `on_demand` / `manual` / `never`
- `status`: `active` / `inactive` / `open` / `archived`
- `project_key`: プロジェクト識別子
- `text`: 実際の文面
- `memory_space_id`: contextual memory の owner（0035）
- `created_session_id` / `last_session_id`: runtime provenance（0035）

`id` / `version` は永続化と競合制御のための必須メタデータであり、`MemoryEntryDto` では省略しない。

`goal` / `now` / `idea` は **standard kind** であり、`scope` / `inject` / `status` は kind ごとに固定される。クライアントが標準 kind に対して異なる `scope` を送った場合は `invalid_request` とする。
`ai mem add` で standard kind 以外の unknown kind を受け付ける場合のみ、クライアント指定の `scope` / `inject` / `status` を許可する。

標準 kind の既定値は次のとおり。

| kind | default scope | default inject | default status |
|------|---------------|----------------|----------------|
| `goal` | `project` | `pinned` | `active` |
| `now` | `session` | `pinned` | `active` |
| `idea` | `project` | `on_demand` | `open` |

`resolve_for_prompt` と通常注入では `goal` / `now` は `active`、`idea` は `open` の entry のみを対象にする。`inactive` / `archived` は保持用であり、自動注入しない。

## 設計概要

### 保存場所

memory の正本は **memory space 配下** の JSONL event store とする（0035 identity split）。

```text
~/.local/share/aibe/memory/spaces/<memory_space_id>/events.jsonl
```

ここで `memory_space_id` は contextual memory の owner である。`AI_SESSION_ID` は directory owner ではなく runtime provenance のみ。

MVP では append-only のイベントログとして扱い、現在状態は replay で導出する。

#### legacy layout（0034 初期・非推奨）

0034 MVP 当初の session 配下 layout は **legacy** として read-through 互換のみ残す。

```text
~/.local/share/aibe/conversations/<AI_SESSION_ID>/memory/events.jsonl
```

新規書き込みは new layout（`memory/spaces/...`）へ寄せ、legacy は破壊しない lazy copy / read-through で扱う。詳細は [0035_aibe-memory-identity-split-spec.md](0035_aibe-memory-identity-split-spec.md)。

### 自動注入の方針

- `goal` は project-scoped として通常の `agent_turn` に自動注入する
- `now` は session scope だが memory space に属し、通常の `agent_turn` に自動注入する
- `idea` は project-scoped で、on-demand のときだけ注入する
- on-demand のトリガは
  - `ai mem` の明示クエリ
  - idea 関連キーワードを含む query

### プロンプトへの載せ方

memory は system instruction にせず、**user-maintained context block** として 1 つの synthetic user message にまとめて注入する。

推奨順は次のとおり。

1. 既存の system instruction
2. shell log tail
3. memory block
4. ユーザーの実 query

これにより、memory は LLM に見えるが system privilege を持たない。

### Budget

- `MemoryEntry.text` の最大長は **8KB**
- `resolve_for_prompt` の注入バジェットは **4KB**

`text` は保存時点で 8KB を超えないことを保証し、`resolve_for_prompt` ではプロジェクト・kind ごとの優先順に従って 4KB 以内に切り詰める。

## protocol

### wire の拡張

`aibe-protocol` に次の RPC を追加する。

- `MemoryApply`
- `MemoryQuery`

どちらも `ClientRequest` / `ClientResponse` の既存 NDJSON フレームに載せる。
`ClientRequest` の memory 系 variant は `deny_unknown_fields` 相当で unknown field を拒否する。

### `MemoryApply`

`ai` から `aibe` へ、memory を保存・更新するための write RPC。

MVP の想定フィールドは次のとおり。

```json
{
  "type": "memory_apply",
  "id": "uuid",
  "session_id": "AI_SESSION_ID",
  "context": {
    "cwd": "/abs/path/to/project",
    "memory_space_id": "ctx_a"
  },
  "operation": {
    "op": "add",
    "kind": "goal",
    "scope": "project",
    "inject": "pinned",
    "status": "active",
    "text": "Ship the parser fix first",
    "make_active": true
  }
}
```

`MemoryApply` の DTO 正本は次のとおり。

シリアライズ上のタグは `op` とし、variant は `Add` / `ClearKind` / `Archive` を使う。

```text
MemoryApply {
  type: "memory_apply",
  id: string,
  session_id: string,
  context: {
    cwd: absolute_path
  },
  operation: MemoryOperationDto
}
```

```text
MemoryOperationDto = Add | ClearKind | Archive

Add {
  op: "add",
  kind: string,
  scope: "project" | "session",
  inject: "pinned" | "on_demand" | "manual" | "never",
  status: "active" | "inactive" | "open" | "archived",
  text: string,
  make_active: bool
}

ClearKind {
  op: "clear_kind",
  kind: string,
  scope: "project" | "session"
}

Archive {
  op: "archive",
  id: string,
  expected_version: u64 | null
}
```

- `expected_version` が指定されていて `id` に対応する entry の `version` と一致しない場合は、`ClientResponse::Error { invalid_request }` envelope で `message: "version_conflict"` を返す。`version_conflict` 専用の新しい ErrorCode は追加しない

```text
MemoryEntryDto {
  id: string,
  memory_space_id: string,
  created_session_id: string,
  last_session_id: string,
  project_key: string | null,
  version: u64,
  created_at_ms: u64,
  updated_at_ms: u64,
  kind: string,
  scope: "project" | "session",
  inject: "pinned" | "on_demand" | "manual" | "never",
  status: "active" | "inactive" | "open" | "archived",
  text: string
}
```

- `project scope` の entry では `project_key` は必須
- `session scope` の entry では `project_key` は `null`

補足:

- `goal set` / `now set` / `idea add` は `op: "add"` の薄いラッパーだが、標準 kind の `scope` / `inject` / `status` はサーバが固定値を決定する。クライアントが異なる値を送った場合は `invalid_request` とする
- `goal set` は project-scoped entry を upsert し、`kind=goal` / `scope=project` / `inject=pinned` / `status=active` / `make_active=true` を使う
- `now set` は session-scoped entry を upsert し、`kind=now` / `scope=session` / `inject=pinned` / `status=active` / `make_active=true` を使う
- `idea add` は project-scoped entry を追記し、`kind=idea` / `scope=project` / `inject=on_demand` / `status=open` を既定とする。`make_active=false` とし、既存の open を変更しない
- `ai mem add` で unknown kind を使う場合のみ、クライアント指定の `scope` / `inject` / `status` を許可する
- `Add` の優先順位は `make_active=true` > `status` である。`make_active=true` のときはサーバが active/inactive の最終状態を決定し、同一 `session_id` + `kind` + `scope` + `project_key`（該当時）の `status=active` entry をすべて `inactive` にする `status_changed` event を記録した上で、新しい entry を `active` で append する。replay 後、同一 `(session, kind, scope, project_key)` に `active` は最大 1 件となる。`make_active=false` のときのみクライアント指定 `status` を採用する
- `goal clear` / `now clear` / `idea clear` は `op: "clear_kind"` で wire できる
- `ai mem clear` は `op: "clear_kind"` で wire でき、必要なら `kind` / `scope` を明示して同じ wire に寄せられる
- 個別 entry を version 付きで無効化したい場合は `op: "archive"` を使う。`expected_version` が指定された場合は、`id` に一致する entry の `version` が一致したときだけ archive を適用する楽観ロックとして扱う
- `text` が 8KB を超える場合は `invalid_request`
- request に `project_key` が含まれていた場合は、`cwd` と整合していても受け付けない
- memory 系 variant は `deny_unknown_fields` 相当で unknown field を拒否する

### `MemoryQuery`

`ai` から `aibe` へ、memory を検索または解決するための read-only RPC。

用途は次の 2 つに分ける。

1. `ai mem` の `list` / `show`
2. `resolve_for_prompt` での prompt 注入解決

`ai mem` の `list` / `show` は query 専用であり、`add` / `clear` は `MemoryApply` に寄せる。

MVP の応答は「該当 entries」と「prompt 用に整形した block」を返す。

```json
{
  "type": "memory_query",
  "id": "uuid",
  "session_id": "AI_SESSION_ID",
  "context": {
    "cwd": "/abs/path/to/project",
    "memory_space_id": "ctx_a"
  },
  "query": {
    "scope": "project",
    "kind": "idea",
    "text": "idea",
    "include_on_demand": true,
    "limit": 20
  }
}
```

`MemoryQuery` の DTO 正本は次のとおり。

```text
MemoryQuery {
  type: "memory_query",
  id: string,
  session_id: string,
  context: {
    cwd: absolute_path
  },
  query: MemoryQueryDto
}
```

```text
MemoryQueryDto {
  scope: "project" | "session" | "all",
  kind: string | "all",
  text: string | null,
  include_on_demand: bool,
  limit: u32 | null
}
```

- `kind` は exact match のフィルタであり、`goal` / `now` / `idea` は予約済み standard kind として扱う

- `project_key` は request に含めない。`cwd` から aibe 側で再導出する
- `MemoryApply` / `MemoryQuery` の memory 系 variant は `deny_unknown_fields` 相当で unknown field を拒否する

### 応答

応答は既存の `ClientResponse` に倣い、`status` と payload を返す。

- `MemoryApplyResult`: 保存・更新した entries と materialized state の要約を返す
- `MemoryQueryResult`: query 結果と prompt block を返す
- 失敗はすべて既存の `ClientResponse::Error { invalid_request }` envelope で返す。`MemoryApplyResult` / `MemoryQueryResult` の `status` は成功時の `ok` のみを持つ
- `version_conflict` も `ClientResponse::Error { invalid_request }` envelope で `message: "version_conflict"` として返す。result payload の `status` で失敗を表現しない

`AgentTurn` の内部解決は wire を経由せず、同じ domain / port 実装を直接使う。

`ClientResponse` の DTO 正本は次のとおり。

```text
MemoryApplyResult {
  type: "memory_apply_result",
  id: string,
  status: "ok",
  result: {
    operation: MemoryOperationDto,
    status: MemoryStatusDto,
    entries: MemoryEntryDto[],
    materialized: {
      goal: MemoryEntryDto | null,
      now: MemoryEntryDto | null,
      ideas: MemoryEntryDto[],
      prompt_block: string
    }
  }
}
```

`entries` は操作で影響を受けた entry 群を返す。`clear_kind` では複数の inactive / archived 化対象をそのまま表現でき、`archive` では単一 entry の状態遷移を返す。

```text
MemoryQueryResult {
  type: "memory_query_result",
  id: string,
  status: "ok",
  result: {
    query: MemoryQueryDto,
    status: MemoryStatusDto,
    entries: MemoryEntryDto[],
    prompt_block: string
  }
}
```

```text
MemoryStatusDto {
  state: "ok" | "empty" | "truncated" | "cleared",
  matched_count: u32,
  applied_count: u32,
  truncated: bool
}
```

## domain / port / adapter

### domain

domain に次を追加する。

- `MemoryKind`
- `MemoryScope`
- `MemoryInjectPolicy`
- `MemoryStatus`
- `MemoryEntry`
- `MemoryResolution`
- `ProjectKey`

`ProjectKey` は `cwd` から導出した正規化済みキーとして扱い、domain 外での path 解決結果をそのまま渡さない。

### port

`aibe` の outbound port に memory 用 interface を追加する。

- append / upsert / clear_kind / archive
- replay / materialize
- project_key による絞り込み
- memory space 単位 store への永続化

port の責務は「保存」と「再構成」であり、prompt block の最終整形は application 側に寄せる。

### adapter

filesystem adapter が `events.jsonl` を管理する。

- ディレクトリは 0700
- `events.jsonl` は 0600
- memory space 単位の lock を使い、競合を書き潰さない
- append-only で event を書く
- replay で current state を再構成する

## AgentTurn 注入

### resolve_for_prompt

`AgentTurn` 実行時、aibe 側で `resolve_for_prompt` を呼び、次の順で prompt を組み立てる。

1. `context.system_instruction`
2. `context.shell_log_tail`
3. memory block
4. ユーザーメッセージ群

`goal` は現在の `cwd` から導出した project_key に対して、`now` は session scope（memory space 内）に対して、`idea` は現在の `cwd` から導出した project_key に対して解決する。`now` は session scope なので project_key の違いで切り替わらない。`goal` / `now` は `active`、`idea` は `open` 以外の entry は通常の prompt 注入対象にしない。

### on-demand の判定

idea の on-demand 注入は、次のどちらかで発火する。

- `MemoryQuery` が明示的に idea を要求する
- query が idea 関連キーワードにマッチする

キーワードは aibe 側の固定 matcher で判定し、LLM による再判定はしない。

### budget の適用

`resolve_for_prompt` は 4KB バジェットを超えないように memory block を作る。

- kind ごとの優先度は `goal` > `now` > `idea`
- 同一 kind では新しいものを優先する
- 途中で budget を超える場合、entry body を UTF-8 boundary を壊さずに部分 truncate する
- block 全体を最後に雑に truncate しない
- truncation marker（`... truncated ...`）は header + marker + footer が budget に入る場合のみ付与する。入らない場合は marker を省略し footer だけ残す
- footer は可能な限り必ず残す

### 0030 との整合

- `route_turn` は memory を見ない
- `AgentTurn` だけが memory を注入する
- これにより smart entry の route 判定と prompt 文脈の責務が分離される

## ai CLI

### MVP の操作

MVP では次の形を許容する。

- `ai goal set <text>`
- `ai goal clear`
- `ai now set <text>`
- `ai now clear`
- `ai idea add <text>`
- `ai idea clear`
- `ai mem add <text>`
- `ai mem list`
- `ai mem show`
- `ai mem clear`

`goal` / `now` / `idea` は `MemoryApply` の薄いラッパーである。

### 振る舞い

| コマンド | 動作 |
|----------|------|
| `ai mem add` | `MemoryApply` を送る。標準 kind ではサーバが `scope` / `inject` / `status` を決定し、クライアントが異なる値を送った場合は `invalid_request` になる。unknown kind を扱う場合のみ、`--kind` / `--scope` / `--inject` / `--status` をクライアント指定で使える。送信する operation は `Add` |
| `ai mem list` | `MemoryQuery` を送る。現在の `cwd` から導出した project_key と session を使って、該当する entries を一覧する |
| `ai mem show` | `MemoryQuery` を送る。現在の `cwd` と session から materialized memory block を表示する |
| `ai mem clear` | `MemoryApply` を送る。`kind` / `scope` / `cwd` に一致する active / open entries を inactive / archived 化する。送信する operation は `ClearKind` |

`ai goal clear` / `ai now clear` / `ai idea clear` は、それぞれ固定 `kind` と `scope` を持つ `ClearKind` の薄いラッパーである。

`goal set` / `now set` / `idea add` は `ai mem add` の薄いラッパーである。`idea add` は `make_active=false` で送信し、既存の open を変更しない。

### aish との関係

`aish` は変更しない。

- `aish` は memory を export しない
- `aish` は memory を保存しない
- `aish` は prompt 組み立てに参加しない

AI_SESSION_ID の供給は既存の smart entry と同じ経路を使う。memory の owner ではなく runtime provenance として request body の `session_id` に載せる。

## セキュリティ

### system instruction に昇格しない

memory は LLM に見せるが、system instruction としては扱わない。これにより、ユーザーが書いた内容を「不可侵の命令」に変えない。

### サイズ制限

- `text` は 8KB を上限とする
- prompt 注入は 4KB に制限する

目的は prompt flooding とログ肥大化の抑制である。

### パス正規化

`project_key` は canonicalize 済みの絶対パスに固定する。

これにより、同じ repo が symlink 経由で複数キーに分岐するのを避ける。
`MemoryApply` / `MemoryQuery` は `cwd` から aibe 側で `project_key` を再導出し、クライアント送信値は採用しない。

### ファイル権限

memory space 配下の store は directory 0700 / `events.jsonl` 0600 を前提にする。

### 秘密情報

memory はユーザー管理のメモであり、secret 自動検出や redaction は MVP の正本ではない。したがって、機密を入れない運用を前提とし、必要なら将来の別仕様で対策する。

## テスト

### unit

- `ProjectKey` 解決が `.git` root 優先で canonicalize されること
- `.git` root が無い場合に `cwd` canonicalize へ fallback すること
- `MemoryEntry.text` が 8KB を超えたら reject されること
- `goal set` が同一 project_key / kind を upsert すること
- `now set` が session-scoped で project_key に依存しないこと
- `idea add` が append-only で、既存 open を変更しないこと
- `Add` の既定 scope / inject / status が標準 kind ごとに一致すること
- unknown kind ではクライアント指定の `scope` / `inject` / `status` を許可すること
- `resolve_for_prompt` が `goal` / `now` を自動注入し、`idea` を on-demand のみ注入すること
- 4KB budget を超える場合に entry body を UTF-8 boundary を壊さず部分 truncate すること
- 極小 budget でも `block.len() <= budget` かつ footer が残ること
- marker は入る余地がある場合のみ付与されること

### protocol

- `MemoryApply` / `MemoryQuery` の serde round-trip
- 不正な `scope` / `inject` / `status` の reject
- 標準 kind に対する非固定 `scope` / `inject` / `status` の reject
- 不正な kind 文字列の reject
- request に `project_key` が含まれた場合の invalid_request
- project scope 操作で `cwd` が欠落していた場合の invalid_request
- `MemoryApply` / `MemoryQuery` の memory 系 variant で unknown field が reject されること
- `ClearKind` と `Archive` の serde round-trip
- `version_conflict` が `ClientResponse::Error { invalid_request }` envelope で `message: "version_conflict"` として返ること

### integration

- Unix socket 経由で `MemoryApply` が保存できること
- `MemoryQuery` が replay 結果を返せること
- `AgentTurn` によって goal / now が自動注入されること
- idea は query が on-demand 条件を満たしたときだけ注入されること
- memory space 単位の lock が同時実行時に競合を書き潰さないこと
- 同時 append が失われないこと
- 0700 ディレクトリ / 0600 ファイル権限で作成されること

### ai

- `ai goal set` / `ai goal clear` / `ai now set` / `ai now clear` / `ai idea add` / `ai idea clear` が wire を通して aibe に届くこと
- `ai mem add` / `list` / `show` / `clear` が期待どおり振る舞うこと
- smart entry の `route_turn` が memory 変更の影響を受けないこと

## 受け入れ条件

1. `aibe` に `memory_space_id` 単位の contextual memory store が追加される
2. 正本は `~/.local/share/aibe/memory/spaces/<memory_space_id>/events.jsonl` に保存される（legacy session layout は read-through 互換のみ）
3. `goal` と `now` は通常の `agent_turn` に自動注入される
4. `idea` は on-demand のみ注入される
5. `ai` に `goal set` / `goal clear` / `now set` / `now clear` / `idea add` / `idea clear` / `mem add` / `mem list` / `mem show` / `mem clear` の導線がある
6. `MemoryApply` / `MemoryQuery` が `aibe-protocol` に追加される
7. `AgentTurn` 時に aibe 側で `resolve_for_prompt` して注入される
8. memory は system instruction ではなく user-maintained context block である
9. `aish` は変更しない
10. 4KB 注入バジェットと 8KB text 上限が守られる
11. `0030` の smart entry と矛盾しない

## 未確定・推測

- 推測: `ai mem` の細かいサブコマンド構成は MVP では `add` / `list` / `show` / `clear` に固定し、`search` は後続でよい
- 推測: idea 関連キーワードの初期セットは aibe 側の固定 matcher で十分であり、LLM 依存の分類は不要
- 推測: `MemoryQuery` の wire 応答は prompt block と raw entries の両方を返す形が実装しやすい
