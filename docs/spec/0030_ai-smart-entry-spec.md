# 0030 — `ai` スマート入口（smart entry）設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-07  
> **関連**: [0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[0028_ai-ux-gap-closure-spec.md](0028_ai-ux-gap-closure-spec.md)、[0029_ai-ux-polish-spec.md](0029_ai-ux-polish-spec.md)、[architecture.md](../architecture.md)、[security.md](../security.md)、[testing.md](../testing.md)

## 目的

`ai '...'` を日常の唯一の入口として扱い、**AI 側（aibe）が意図を判定し、必要なら許可を求めて tools / preset / 会話継続を選ぶ** 体験を v1 として定義する。

本書の正本は次の 3 点である。

1. 入口は `ai '...'` を常時 ON とする
2. ルーティングは aibe 側の軽量 LLM 分類 `route_turn` に集約する
3. 会話状態は aibe が disk に保持し、`AI_SESSION_ID` を共有単位とする

## 非目標

- `ai` から LLM を直接呼ぶこと
- `aish` にルーティングや会話状態管理を追加すること
- スマート入口の opt-out フラグを v1 で導入すること
- 0027/0028 の local transcript 方針をそのまま温存すること
- aibe の router を LLM 以外の heuristic のみで構成すること
- Windows 対応

## 0027 / 0028 / 0029 との関係

### 0027 からのパラダイム変更

0027/0028 では、`ai chat` の transcript を client-side に閉じ、`ai` が履歴を積み上げる方針だった。0030 ではこれを **aibe が full transcript を disk 保持する方針** に切り替える。

この変更により、`ai` は「会話の所有者」ではなくなり、次の役割に縮退する。

- 入口の正規化
- `AI_SESSION_ID` の受け渡し
- route 結果の適用
- 承認 UI
- local history への索引記録

`ai` 側の client-side transcript は v1 の正本ではない。以後の会話継続の正本は aibe の conversation store である。

### 0028 との関係

0028 で確定した `conversation_id`、`history` 記録、`shell_exec_approval` の優先順位、`--dry-run` の秘匿規則は継承する。

ただし、`conversation_id` の保管場所は aibe 側の transcript store に移る。local history は追跡と再実行のための索引にとどめる。

### 0029 との関係

0029 で自動検証した `--yes-exec`、history GC、streaming mock の考え方は継承する。0030 ではそれらを smart entry の経路に組み込む。

- `--yes-exec` は smart entry でも同じ優先順位を持つ
- history GC の設計はそのまま使う
- streaming 検証は route / conversation 継続の統合テストに再利用する

## 要約

`ai '...'` の実行時、`ai` は tty / 非 tty を判定し、必要なら aibe の `route_turn` RPC を 1 回実行する。route が失敗した場合は 1 回だけ再試行し、それでも失敗すれば **床（`tools=[]` の 1 shot ask）** に落として継続する。

`route_turn` は軽量 LLM 分類であり、会話の継続可否、推奨 preset、推奨 tools、log tail 増量提案、`new_conversation` フラグを返す。`ai` はこれを **advisory** として扱い、CLI 明示値があればそれを優先する。

`AI_SESSION_ID` は `aish` が export する。`aish` 外で `ai` が起動された場合は `ai` が自前生成する。`AI_SESSION_ID` が同じ複数 tab / process は、aibe の conversation store を共有する。

## クレート境界

| クレート | 責務 |
|---------|------|
| **aish** | `AI_SESSION_ID` を export するだけ。スマート入口、route、会話保持、承認 UI は持たない |
| **ai** | `route_turn` の orchestration、CLI 明示値の適用、TTY fallback、承認 UI、local history の索引記録 |
| **aibe** | `route_turn` 実装、conversation store、full transcript の永続化、`conversation_id` 生成、route summary の生成 |
| **aibe-protocol** | `RouteTurnRequest` / `RoutePlan` / conversation 契約の wire 定義 |

依存方向は既存の境界を壊さない。`ai` は `aibe` 本体へ path 依存を増やさず、`aibe-client` と `aibe-protocol` を経由する。

## Smart Entry の動作

### 入口の規則

- `ai '...'` は smart entry を使う
- smart entry は v1 で常時 ON とする
- `ai --new '...'` は新しい会話を要求する
- `--new` は継続判定のみをスキップし、`route_turn` 自体は実行する
- 非 TTY（pipe / CI / stdin 非 tty）の場合、smart entry は無効化し、従来の 1 shot ask に落とす

### 標準フロー

1. `ai` が起動時に `AI_SESSION_ID` を解決する
2. `ai` が `route_turn` に `query`、`cwd`、session 情報、`recent_summary` を渡す
3. aibe が `RoutePlan` を返す
4. `ai` は CLI 明示値を RoutePlan より優先して最終 request を組み立てる
5. 需要があれば `shell_exec` 承認 UI を session 単位で出す
6. aibe は full transcript と `conversation_id` を disk に記録する

### 非 TTY fallback

非 TTY では smart entry を使わない。これは安全性と再現性を優先した設計であり、ルーターの可視化や承認 UI が成立しない環境では状態遷移を増やさない。

## ルーター

### 方式

- 方式は **軽量 LLM 分類** とする
- 呼び出しは 1 回あたり 1 回のみ
- 出力は短い JSON に限定する
- 配置は aibe 側の新 RPC `route_turn`
- profile は config の `[router] profile = "fast"` を使う

### 入力

`RouteTurnRequest` が受ける情報は次を含む。

- `query`: ユーザー入力の生テキスト
- `cwd`: クライアントの絶対パス
- `session`: `AI_SESSION_ID`、`AISH_SESSION_DIR` の有無、tty / non-tty、`--new` の有無
- `recent_summary`: 直近 1 turn の要約
- `cli_overrides`: `--preset`、`--tools`、`--log-tail`、`--yes-exec`、`--new` などの明示値

`recent_summary` は aibe が生成する。`ai` は履歴全体を持たず、要約だけを要求する。

### `RouteTurnRequest` 契約

```text
RouteTurnRequest {
  id: uuid,
  query: string,
  cwd: absolute_path,
  session: {
    ai_session_id: string,
    aish_session_dir: string | null,
    tty: bool
  },
  conversation: {
    conversation_id: string | null,
    recent_summary: string | null,
    new_conversation: bool
  },
  cli_overrides: {
    preset: string | null,
    tools: string[] | null,
    log_tail_bytes: u64 | null,
    yes_exec: bool
  }
}
```

- `query` と `cwd` は必須
- `conversation_id` は既存会話の継続時にのみ入る
- `recent_summary` は空でもよい
- `new_conversation=true` の場合、継続判定は行わないが route 自体は行う

### `RoutePlan` 契約

```text
RoutePlan {
  conversation_id: string,
  new_conversation: bool,
  route_kind: "one_shot" | "chat" | "continue" | "tool_assisted",
  recommended_preset: string | null,
  recommended_tools: string[] | null,
  log_tail_bytes: u64 | null,
  require_shell_approval: bool,
  log_tail_escalation: bool,
  route_reason: string,
  confidence: number | null
}
```

- `RoutePlan` は **advisory** である
- ここでの advisory は `route_advisory=true` と同義であり、`ai` は必ず CLI 明示値を最優先にして最終 request を組み立てる
- `conversation_id` は aibe が最終決定する
- `recommended_preset` と `recommended_tools` はあくまで提案であり、CLI 明示値が優先される
- `route_reason` は stderr / history では redacted した形で扱う
- `log_tail_escalation=true` の場合、エラー調査系クエリとして log tail 増量を提案していることを示す
- `log_tail_escalation` は昇格扱いなので、必要なときだけ stderr に出す

### `conversation_id` 契約

- `conversation_id` は aibe が生成する
- 同じ `AI_SESSION_ID` の複数 tab は同じ conversation store を共有できる
- ただし `--new` が指定された turn は新しい `conversation_id` を割り当てる
- `ai chat` と暗黙継続は同じ `conversation_id` を共有可能
- `ai history` には `conversation_id` を記録する

### 失敗時の挙動

`route_turn` が decode 失敗、timeout、LLM error、内部 error で失敗した場合は、1 回だけリトライする。

それでも失敗したら次の safe fallback に落とす。

- `tools=[]`
- 1 shot ask
- `stderr` に警告
- `history` には fallback であることを記録

これは **fail-open ではなく safe fallback** であり、route の判断失敗をユーザーの追加承認なしに危険側へ寄せないための措置である。

## preset vs dynamic

v1 では dynamic router を正本とし、preset は副次的な明示調整として扱う。

| 観点 | dynamic 主 | preset 主 |
|------|------------|----------|
| 意図の把握 | 現在の query と recent_summary を見て都度変えられる | 固定パターンに強い |
| 運用 | 変化するタスクに追従しやすい | 再現性は高い |
| 誤設定耐性 | route が補正しやすい | ユーザー設定の影響が強い |
| 透明性 | route_reason を出せる | 事前設定依存で理由が固定化しやすい |
| 事故面 | 誤分類の影響が出る | preset 固着の事故が出る |

### v1 正本

**v1 の正本は dynamic 主、preset 副 optional** とする。

- `RoutePlan` は dynamic の判断を返す
- preset はユーザーが明示したときだけ強く作用する
- route が preset を提案しても、CLI 明示値があれば CLI が勝つ
- preset は「固定の運用ポリシー」や「既知の作業モード」を表す補助線として残す

## CLI 明示値との関係

`--preset` / `--tools` / `--log-tail` などの明示指定がある場合、`route_turn` は常に実行されるが、最終値は CLI が上書きする。

優先順位は次の通り。

1. CLI 明示値
2. RoutePlan の提案
3. config 既定
4. hardcoded default

`--new` は継続判定のみを止める。`route_turn` を省略するのではなく、`RoutePlan.new_conversation=true` を強制する入力として扱う。

## tools と承認

### hardcoded 既定

`[ask].tools = []` を床とする。つまり、smart entry を使っても、まずは text-only の 1 shot ask を既定とする。

### shell 昇格

- read-only の床から shell が必要になった場合、`shell_exec` 承認を求める
- 承認は session 単位で行う
- `AISH_SESSION_DIR` をスコープにして yes/n の記憶を持つ
- aish 外では TTY fallback を使い、暗黙 chat と session shell 許可を tty 単位で扱う

### yes-exec 連携

`--yes-exec` は 2 段プロンプトにする。

1. session shell 許可 `y/N`
2. 「以降同一コマンド自動承認」 `y/N`

この 2 段構成は、1 回の操作と session 内の反復操作を分けて扱うためのもの。`shell_exec_approval=never` はこれを上書きしない。

### 優先順位

`shell_exec_approval` の優先順位は 0028/0029 と同じで、クライアント側が `never` を越えてはいけない。

1. aibe config の `never`
2. CLI / preset
3. session 許可

`never` は最上位の拒否であり、`--yes-exec` でも越えない。

## 透明性

次の場合だけ stderr に表示する。

- route によって会話が継続されたとき
- `new_conversation` によって会話が切り替わったとき
- tools や preset の昇格があったとき
- shell 承認が必要になったとき
- route が失敗して fallback したとき
- `log_tail_escalation=true` によって log tail 増量が提案されたとき

通常の one-shot ask のまま終わる場合は、追加の route 詳細を stderr に出さない。これは UX のノイズと情報露出を減らすためである。

## セキュリティ

### 1. `never` / non-TTY / fail-closed の優先順位

| 優先順位 | 条件 | 挙動 | 補足 |
|----------|------|------|------|
| 1 | `shell_exec_approval=never` | 絶対拒否 | `--yes-exec` でも越えない |
| 2 | non-TTY | smart entry 無効、承認 UI 無効、1 shot ask | pipe / CI での暗黙昇格を避ける |
| 3 | fail-closed | route / prompt / decode が怪しい場合は昇格しない | 安全側の床に落とす |

### 2. `AI_SESSION_ID` の安全性

- `aish` は `AI_SESSION_ID` を export する
- `aish` 外では `ai` が `AI_SESSION_ID` を生成する
- session id の使い回しは conversation 共有のためであり、権限の共有ではない
- conversation store は 0600、ディレクトリは 0700 相当で作る
- 同じ `AI_SESSION_ID` の複数 tab は transcript を共有できるが、別ユーザーとは共有しない

### 3. tab 共有の意味

同じ `AI_SESSION_ID` を持つ複数 tab は、**同一 conversation の続きとして扱われる**。

これは次を意味する。

- 片方での user / assistant 履歴が、もう片方の次 turn に見える
- 片方の `--new` は、もう片方の既存 conversation とは別系統を作る
- 共有は convenience であり、権限昇格ではない

### 4. transcript の保持

- full transcript は aibe が disk に保持する
- local history には redacted 索引のみを置く
- raw transcript を ai の history index に複製しない
- route_reason、recent_summary、tool 提案は redaction を前提に記録する
- local history には redacted された `RoutePlan` 全体を記録し、後から route の判断を追えるようにする

## aibe-protocol

### RPC

`route_turn` は新 RPC とする。

#### Request

- `type = "route_turn"`
- `id`: 相関 ID
- `query`: 生の user input
- `cwd`: 絶対パス
- `session`: `AI_SESSION_ID` と tty 情報
- `conversation`: `conversation_id`、`recent_summary`、`new_conversation`
- `cli_overrides`: `preset`、`tools`、`log_tail_bytes`、`yes_exec`

#### Response

```text
RouteTurnResult {
  id: uuid,
  status: "ok",
  plan: RoutePlan
}
```

エラー時は既存 error envelope を使う。`route_turn` 専用の例外型は増やしすぎない。

## 構成・保存

### conversation store

`aibe` は full transcript を次の場所に保存する。

```text
~/.local/share/aibe/conversations/
  <AI_SESSION_ID>/
    index.jsonl
    conversations/
      <conversation_id>.json
```

- `index.jsonl` は redacted 索引
- `conversations/<conversation_id>.json` は full transcript と summary を含む
- すべてのファイルは 0600 相当で作成する
- session ディレクトリは 0700 相当とする
- 同一 `conversation_id` への書き込みは aibe 内で排他制御し、複数 tab から同時に更新されても transcript が壊れないようにする
- 保存は原子的に行い、途中失敗時は旧版を残す

### 移行

0027/0028 の client-side transcript からの **一括バックフィルは行わない**。v1 は lazy creation とし、新規 turn から aibe store を source of truth にする。

既存の local history は `retry` / `rerun` / `history` のために残し、必要な場合のみ既存レコードの `conversation_id` を参照する。

## 受け入れ条件

### unit

- `route_turn` の request / response 形状が serde で固定される
- `RoutePlan` が advisory であり、CLI 明示値が勝つことを固定する
- `AI_SESSION_ID` の解決が `aish` 由来 / `ai` 生成の両方で安定する
- conversation store が 0600 / 0700 で作られる
- non-TTY では smart entry が無効になる

### integration

- `ai '...'` が tty で `route_turn` を経由し、同じ `AI_SESSION_ID` の複数 tab で conversation を共有できる
- `ai --new '...'` が `route_turn` を実行しつつ、新しい conversation を作る
- `--preset` / `--tools` を指定しても `route_turn` が常に呼ばれ、CLI 明示値が RoutePlan を上書きする
- `route_turn` 失敗後の 1 回リトライが行われ、それでも失敗したら `tools=[]` の 1 shot ask に落ちる
- `shell_exec_approval=never` が `--yes-exec` より強い

### manual

- `ai '...'` を複数 tab で実行して同じ conversation が継続される
- `ai --new '...'` が新規 conversation になる
- pipe / CI では smart entry が無効になり、従来の 1 shot ask に落ちる
- shell 承認は session 単位で記憶され、`never` を越えない

## テスト方針

| 種別 | 対象 |
|------|------|
| unit | `RouteTurnRequest` / `RoutePlan` serde、`AI_SESSION_ID` 解決、conversation store パーミッション、CLI override の優先順位 |
| integration | `ai '...'` の smart entry、`--new`、route retry、fallback、`--yes-exec` / `shell_exec_approval` の優先順位 |
| aibe integration | `route_turn` の JSON 返却、conversation store 永続化、full transcript 共有 |
| manual | 複数 tab 共有、non-TTY fallback、承認 UI の見え方 |

`docs/testing.md` と `docs/manual/` は実装時に同じ変更で更新する。

## 未確定事項

### 推測

- `recent_summary` の具体的な要約アルゴリズムは aibe 内部実装に委ねる。v1 の正本は「直近 1 turn の要約が `route_turn` に渡ること」であり、要約生成手法そのものではない。
- `RoutePlan.route_reason` の文面は、ユーザー表示に必要な最小限の redaction を守る前提で、内部実装で整える。

### 非目標として固定

- 0027/0028 の client-side transcript を復活させること
- 一括 migration job で既存 history をすべて aibe store に書き換えること
- non-TTY での smart entry 継続
- `never` を越える自動承認
