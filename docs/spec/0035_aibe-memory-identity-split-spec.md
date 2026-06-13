# 0035 — AIBE Memory Identity Split 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-12  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[0034_aibe-contextual-memory-spec.md](0034_aibe-contextual-memory-spec.md)、[0037_aibe-contextual-memory-runtime-v1-spec.md](0037_aibe-contextual-memory-runtime-v1-spec.md)、[0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)、[0019_aish-session-log-integration-spec.md](../done/0019_aish-session-log-integration-spec.md)

### 0.1 0037 との関係

本書は **MVP 設計書** として残す（履歴・背景参照用）。[0037](0037_aibe-contextual-memory-runtime-v1-spec.md) が **Contextual Memory Runtime v1 の正式正本** である。本書と 0037 が矛盾する場合は **0037 を優先**する。

## 目的

`AIBE` の contextual memory 系の正本を `AI_SESSION_ID` から切り離し、`memory_space_id` を所有者として扱う。  
これにより、短命な runtime session が変わっても contextual memory が失われず、同一 `memory_space_id` を共有する別セッションから同じ goal / idea / decision / rule を参照できるようにする。

本書の目的は次の 3 点である。

1. `AI_SESSION_ID` と `memory_space_id` を分離する
2. `AI_SESSION_ID` を shell log / conversation / runtime provenance に限定する
3. `memory_space_id` を contextual memory の正本 ID として扱う

## 非目標

- `aish` のログ寿命や保存レイアウトを memory の正本にすること
- `AI_SESSION_ID` を memory の主キーとして維持すること
- 既存の session-scoped memory を単に名前だけ変えて残すこと
- Windows 対応
- すべての memory 種別を一度に完成させること
- `decision` / `rule` の高機能 UI をこの段階で完成させること

## 背景

0034 の MVP では memory の正本が `conversations/<AI_SESSION_ID>/memory/events.jsonl` にあり、`session_id` がそのまま owner だった。  
この設計は短命な shell / conversation session には素直だが、contextual memory を本格化すると次の問題が出る。

- session が切り替わると goal / idea / now の連続性が失われる
- 同じ作業コンテキストでも、再接続や再起動で memory が別物に見える
- shell log の寿命と memory の寿命が同一視される

本書では、runtime session と memory space を別の identity として扱う。  
`AI_SESSION_ID` はその turn の provenance と対話の継続に使い、`memory_space_id` は context の継続に使う。

## 用語

### AI_SESSION_ID

runtime session の ID。shell log、conversation、request provenance を指す。  
memory の owner ではない。

### memory_space_id

contextual memory の正本 ID。session が変わっても同じ space を参照できる。  
goal / idea / decision / rule など、継続すべき context の owner になる。

### AIBE_CONTEXT_ID

user-facing な context 名。**クライアント `ai` のみ**が参照する環境変数。  
MVP では path-safe かつ stable な `memory_space_id` と同値として扱う。display label の分離は後続仕様に回す。  
**`aibe` デーモンはサーバ側の `AIBE_CONTEXT_ID` を読まない。**

### project_key

`cwd` から導出する内部の project 識別子。wire には載せない。  
`AIBE_CONTEXT_ID` が無いときの **クライアント側** project-backed `memory_space_id` 生成に使う。

### legacy_session_<session_id>

`memory_space_id` が明示されない場合の暫定 fallback ID。非推奨。  
0035 の互換性維持のために残すが、正本の扱いではない。

## 設計概要

### identity split

本書の中心決定は次のとおり。

- `AI_SESSION_ID` は runtime provenance
- `memory_space_id` は contextual memory ownership
- `cwd` は project-backed memory space の解決材料

この分離により、別 `AI_SESSION_ID` から同じ `memory_space_id` を見に行ける。  
逆に、同じ `AI_SESSION_ID` でも `memory_space_id` が違えば別 context として扱う。

### 参照の優先順位

memory space の解決は **クライアント側** と **サーバ側** で責務を分ける。

#### クライアント側（`ai` の context resolution）

`ai` は RPC / turn 送信前に `memory_space_id` を解決し、`MemoryContext.memory_space_id` または turn `context.memory_space_id` に載せる。優先順位は次のとおり。

1. `AIBE_CONTEXT_ID` 環境変数（**クライアントプロセス**の env。`ai` が起動したシェル等）
2. `~/.config/ai/config.toml` の `[context] current`（`ai context use/new`）
3. `cwd` から導出した project-backed `memory_space_id`（`project_<hash>`）
4. `legacy_session_<session_id>`（非推奨）

`AIBE_CONTEXT_ID` は **client-side resolution** としてのみ扱う。`ai context current` の表示もこの順序に従う。

#### サーバ側（`aibe` daemon の fallback）

**`aibe` デーモンはサーバ環境変数 `AIBE_CONTEXT_ID` を読まない**（複数クライアント接続時にサーバ env で全員の context が変わるのを防ぐ）。

request に `memory_space_id` が載っていない旧クライアント向けに、`aibe` は次の **server-side fallback** のみを行う。

1. request に明示された `memory_space_id`（`MemoryContext` または turn `context`）
2. `cwd` から導出した project-backed `memory_space_id`
3. `legacy_session_<session_id>`

`ai` は可能な限りクライアント側で 1 〜 4 を解決して送る。`aibe` は 2 〜 3 で旧 request を救済する。

### project-backed id の生成

`cwd` から `project_key` を求め、その project に安定した `memory_space_id` を割り当てる。  
`memory_space_id` は path component として安全である必要があるため、raw path をそのまま使わず、`project_key` から安定した canonical ID を生成する。

> 推測: project-backed `memory_space_id` は `project_<stable-hash(project_key)>` のような path-safe 文字列にするのが最も実装しやすい。  
> raw の `project_key` は metadata 側に残し、filesystem の directory 名には使わない。

### now の扱い

`now` は work/current context を表すが、identity split 後は `memory_space_id` に属する。  
ただし `goal` / `idea` よりも「今の作業状況」という性質が強いので、表示と注入では stale 判定を持たせる。

- `now` は memory space からは消さない
- `now` は session が変わったら stale として扱える
- `now` の stale は display / prompt 注入で明示する

MVP の stale 判定は「最後に更新した session と現在の session が異なる」だけで十分とする。  
wall-clock TTL は将来拡張とする。

## protocol 変更

### MemoryContext 拡張

`aibe-protocol` の `MemoryContext` を拡張する。

```text
MemoryContext {
  cwd: absolute_path | null,
  memory_space_id: string | null
}
```

- `session_id` は request body レベルに残す
- `cwd` は任意。project scope の apply/query では cwd 必須。session / global scope の apply/query では cwd なし可
- cwd が無い AgentTurn / 旧 request では server-side fallback により legacy session space を使う
- `memory_space_id` は nullable にする
- unknown field は引き続き reject する

### MemoryEntry 変更

`MemoryEntry` に `memory_space_id` を追加する。

```text
MemoryEntry {
  id: string,
  created_session_id: string,
  last_session_id: string,
  memory_space_id: string,
  kind: string,
  scope: "project" | "session" | "global",
  inject: "pinned" | "on_demand" | "manual" | "never",
  status: "active" | "inactive" | "open" | "archived",
  text: string,
  project_key: string | null,
  created_at_ms: u64,
  updated_at_ms: u64,
  version: u64
}
```

- `open`: 未整理・未処理の memory（`idea` の既定 status）

`created_session_id` は entry を最初に作った runtime session、`last_session_id` は最後に更新した runtime session を表す。  
`session_id` を owner として再利用しないことで、provenance と ownership を分離する。

> 推測: event 側には `created_session_id` と `last_session_id` の両方を持たせ、`now` の stale 判定は `last_session_id` を使うのが最も実装しやすい。

### MemoryRequest の意味

`MemoryApply` / `MemoryQuery` の request body は `session_id` をそのまま残す。  
これは runtime session の provenance であり、memory ownership ではない。

- `session_id`: runtime session provenance
- `context.memory_space_id`: memory ownership
- `context.cwd`: project-backed memory space 解決材料

### error / validation

次のエラーは従来通り invalid_request で返す。

- `cwd` が absolute_path ではない、または canonicalize 不能な場合
- `memory_space_id` の解決に失敗した場合
- standard kind に対して固定値以外の `scope` / `inject` / `status` を送った場合
- unknown field が含まれる場合

`legacy_session_<session_id>` は暫定 fallback なので、明示的に無効化する方針に進む場合は後続仕様で切る。

## domain 変更

### 追加 / 変更される責務

domain は次の identity モデルを持つ。

- `MemorySpaceId`
- `ProjectKey`
- `MemoryEntry`
- `MemoryResolution`
- `MemoryFreshness` または同等の stale 表現

`MemoryEntry` は `memory_space_id` を owner として持つ。  
`session_id` は provenance に降格する。

### resolver の責務

resolution は **クライアント** と **サーバ** で分ける。

**クライアント（`ai`）**:

1. `cwd` から `project_key` を求める（project-backed id 生成用）
2. `AIBE_CONTEXT_ID` > local config > project-backed id > legacy session の順で `memory_space_id` を解決する
3. 解決済み ID を RPC / turn に載せる

**サーバ（`aibe`）**:

1. request の明示 `memory_space_id` を優先する
2. 未指定時は `cwd` から project-backed `memory_space_id` を導出する
3. それも無ければ `legacy_session_<session_id>` に fallback する

domain は raw path を owner key として扱わない。  
path 解決は adapter か composition root で行い、domain には正規化済みの値だけを渡す。

### now の stale 表現

`now` は `memory_space_id` に保存されるが、prompt 注入時に stale であることを示せる必要がある。  
domain では次の判断材料を持つ。

- `current_session_id`
- `last_session_id`
- `updated_at_ms`

MVP では `last_session_id != current_session_id` を stale の主判定とする。  
この判定結果は `MemoryResolution` に含める。

## store レイアウト

### primary layout

保存先を session 配下から memory space 配下へ移す。

```text
~/.local/share/aibe/memory/spaces/<memory_space_id>/events.jsonl
```

ここで `memory_space_id` は owner の canonical ID である。  
session は directory の owner ではない。

### legacy fallback

0034 までの旧 layout は次のとおりだった。

```text
~/.local/share/aibe/conversations/<AI_SESSION_ID>/memory/events.jsonl
```

0035 では新 layout を正とするが、互換のために次を許容する。

- `memory_space_id` が未指定なら `legacy_session_<session_id>` を暫定生成
- 既存の legacy data は read-through で参照できる
- 新規書き込みは可能な限り new layout に寄せる
- `ai context use/new` で新しい名前付き context を選んだ場合、その context が初回に legacy data を見つけたら new layout へ lazy copy して以後は `memory_space_id` 側を正本にする。これにより 0034 の既存データを破壊せず、同じ context 名へ複数 session から継続できる

> 推測: 既存 legacy data を完全移行する batch job より、初回アクセス時の read-through + lazy write のほうが実装コストと安全性の両面で妥当。

### file ownership

- directory は 0700
- `events.jsonl` は 0600
- session ではなく memory space 単位で lock を取る

### event schema

event log 自体は append-only を維持する。  
ただし event には memory space id と provenance が必要になる。

- `Added`: entry 本体に `memory_space_id` を含める
- `StatusChanged`: どの memory space に属する entry かを replay 時に判定できる必要がある

replay は `memory_space_id` ごとに current state を導出する。

## ai CLI

### context コマンド

最小 MVP として次のコマンドを追加する。

- `ai context current`
- `ai context use <NAME>`
- `ai context new <NAME>`

`<NAME>` は path-safe な context id であり、そのまま `memory_space_id` として扱う。  
人間向けの別名や表示ラベルは MVP の対象外とする。

### 振る舞い

| コマンド | 挙動 |
|----------|------|
| `ai context current` | **クライアント側**の解決結果を表示する。優先順位は `AIBE_CONTEXT_ID` > local config > cwd project-backed id > legacy session |
| `ai context use <NAME>` | local config の current context を `<NAME>` に変更する。`AIBE_CONTEXT_ID` があればそちらが優先される |
| `ai context new <NAME>` | 新しい context を作る。MVP では「新規登録 + current に設定」の意味でよく、実体の directory は最初の memory write で lazy 作成してよい |

### local config

`ai` は local 設定に current context を保存できるようにする。  
既存の `~/.config/ai/config.toml` を再利用し、context 用の小さなセクションを追加するのが最小差分である。

> 推測: 既存の `ai` 設定ファイルに `[context] current = "..."` を追加する形が、`ai context use` の実装と `current` の解決を最も単純にできる。

### memory operation への反映

`ai` は memory 系 RPC を送るとき、**クライアント側で解決済み**の `memory_space_id` を `MemoryContext` に載せる。

優先順位（クライアント側 resolution）:

1. `AIBE_CONTEXT_ID`
2. local config の current context
3. cwd から導出した project-backed id
4. `legacy_session_<session_id>`

`cwd` は project-backed id の生成に使うが、**常に必須ではない**。project scope の apply/query では cwd 必須。session / global scope では cwd なし可。  
`aibe` はサーバ env `AIBE_CONTEXT_ID` を読まず、request 明示 ID > cwd project > legacy session のみで fallback する。

### display

`ai context current` や `ai mem show` は、`memory_space_id` と provenance を分けて表示する。  
`now` が stale の場合は、その旨を明示する。

## 互換・移行

### backward compatibility

0034 以前のクライアントや旧 request は `memory_space_id` を送らない。  
その場合でも、`aibe` は `legacy_session_<session_id>` にフォールバックして動作を止めない。

### forward compatibility

`memory_space_id` を理解しないクライアントに対しては、従来の session-scoped 行動に見える。  
ただし新しい正本は new layout に置かれるので、将来的には session ownership を縮退させる。

### migration posture

本仕様では強制移行を要求しない。  
まずは新しい ID モデルを導入し、旧 data は read-through で扱う。

### kind の拡張

0035 は `goal` / `idea` / `now` だけで完結しない。  
将来の `decision` / `rule` は `memory_space_id` の配下に同居できる前提で設計する。

## テスト

### protocol

- `MemoryContext` が `memory_space_id` と `cwd` を受け取れること
- unknown field を引き続き reject すること
- `MemoryEntryDto` に `memory_space_id` が含まれること
- `created_session_id` / `last_session_id` が round-trip でき、owner と provenance が分離されること

### domain

- `cwd` から project-backed `memory_space_id` を導出できること
- **クライアント側**で `AIBE_CONTEXT_ID` が project-backed id より優先されること
- **サーバ側**では `AIBE_CONTEXT_ID` env を読まないこと
- server-side fallback（request 明示 > cwd project > legacy session）が働くこと
- legacy data を named context に lazy copy した後、別 session から同じ `memory_space_id` を参照できること
- `now` が別 session で stale と判定されること

### store

- `memory/spaces/<memory_space_id>/events.jsonl` に保存されること
- 同じ `memory_space_id` を別 `AI_SESSION_ID` から参照して同じ goal を見られること
- `ctx_a` を共有する `sess_001` と `sess_002` が同じ memory を見ること
- `ctx_b` を持つ `sess_003` が `ctx_a` の goal を見ないこと
- legacy fallback で `memory_space_id` 未指定 request が落ちないこと
- legacy data が new layout に lazy copy された後も元の session store を壊さないこと

### ai CLI

- `ai context current` が current resolution を表示すること
- `ai context use <NAME>` が local config を更新すること
- `ai context new <NAME>` が新しい current context を作ること
- `ai goal set` / `show` / `clear` が `memory_space_id` を通して同じ context を参照すること

## 受け入れ条件

1. `AI_SESSION_ID` が memory の owner ではない
2. `memory_space_id` が contextual memory の owner である
3. クライアント側 `AIBE_CONTEXT_ID` で同じ memory space を複数 session から共有できる
4. `cwd` から project-backed memory space を導出できる
5. `memory_space_id` 未指定時は legacy session fallback がある
6. `MemoryContext` と `MemoryEntry` が split 後の意味を表現できる
7. `ai context current/use/new` が最小 MVP として動く
8. `now` は work/current context として stale を扱える
9. goal / idea / decision / rule の正本は session ではなく memory space に属する
10. 既存の 0034 で保存されたデータを破壊しない

## リスク

- `memory_space_id` の canonicalization を誤ると、同じ context が別物に見える
- legacy fallback を長く残すと、session ownership が事実上温存される
- `now` の stale 表示を弱くすると、現在作業中の状態が古いまま見える
- project-backed id の生成を raw path でやると、filesystem 安全性と可搬性が悪化する
- 既存 data の read-through 互換を軽視すると、0034 からの移行で見かけ上の data loss が起きる

## docs 反映方針

実装時には次を同じ変更で更新する。

- `docs/architecture.md` の memory 節
- `docs/testing.md` の memory / context テスト方針
- `docs/security.md` の memory ownership と stale / leakage の扱い
- `docs/manual/` の context 運用手順

本書はその前提となる設計の正本である。
