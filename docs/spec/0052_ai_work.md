# AISH `ai work` 作業文脈管理仕様書

## 1. 背景

現在の AISH には Contextual Memory 系の機能として、概ね以下の低レベル操作が存在する。

```text
ai goal ...
ai now ...
ai idea ...
ai mem ...
ai context ...
```

しかし現状では、これらは単体のメモリ操作としては機能しているものの、日常的な作業導線としては使われにくい。

理由は以下の通り。

* `goal` / `now` / `idea` をユーザーが毎回明示的に使うメンタルモデルがない
* 単発の `ai ...` 質問では、作業文脈を登録する動機が弱い
* 複数作業・脱線・一時中断・後回しを扱う導線がない
* トップレベルに `start` / `status` / `resume` などの動詞を増やすと、何に対する操作か分かりづらい
* Contextual Memory の本質は「記憶」ではなく「作業継続」である

そこで、既存の Contextual Memory を直接置き換えるのではなく、その上に高レベルUXとして `ai work ...` を追加する。

## 2. 目的

`ai work` は、AISH における「現在の作業面」を管理するためのコマンド群である。

目的は以下。

1. 現在の作業目的、注目点、決定事項、思いつき、後回し項目を扱えるようにする
2. 複数作業を切り替えられるようにする
3. 一時的な寄り道を stack として扱い、元の作業に戻れるようにする
4. 現在の作業に関係ない思いつきを defer/backlog に逃がせるようにする
5. 既存の `goal` / `now` / `idea` / `mem` / `context` を低レベルAPIとして活かす
6. 将来、エディタ・ブラウザ・replay・git 状態などが同一 work context を参照できる土台にする

## 3. 非目的

今回の実装では以下は行わない。

* 完全なタスク管理ツール化
* カンバン、期限、担当者、優先度などのプロジェクト管理機能
* P2P aibe 共有
* ブラウザ拡張・エディタ拡張の実装
* 汎用 client-side tool protocol の本格実装
* LLM による自動 memory 更新の全面導入
* 複雑な DAG 型 work graph

今回作るのは、あくまで Contextual Memory を日常利用しやすくするための最小作業文脈レイヤーである。

## 4. 基本概念

### 4.1 Work

Work は、ユーザーが現在取り組んでいる作業単位である。

例:

```text
Contextual memory の運用導線を設計する
replay履歴をLLMから参照できるようにする
Smart Preprocessor の観測ログを設計する
```

Work は goal / focus / notes / ideas / decisions / deferred items を持つ。

### 4.2 Active Work

現在アクティブな作業。
通常の `ai ...` 実行時に注入される Contextual Memory は、active work を基準にする。

active work は原則1つだけ。

### 4.3 Work Stack

一時的な寄り道用の stack。

現在の作業から派生して短時間だけ別作業を行い、その後元の作業へ戻る用途で使う。

例:

```text
本線:
  Contextual memory の運用導線を設計する

寄り道:
  ai work のサブコマンド命名を整理する
```

この場合、`push` で寄り道を開始し、`pop` で元の作業へ戻る。

### 4.4 Deferred Work / Backlog

今はやらないが忘れたくない作業候補。

現在の active work には割り込まない。
`idea` と異なり、現在作業の中の案ではなく、別作業として後回しにする対象である。

### 4.5 Idea

現在の work に関係する未確定案。

例:

```text
replay tool は read-only に限定した方がよさそう
```

### 4.6 Decision

現在の work における決定事項。

例:

```text
最初は汎用 client-side tool protocol ではなく replay 専用 tool で検証する
```

### 4.7 Focus

現在の work の中で、今まさに注目している点。
既存の `now` に相当する。

## 5. コマンド体系

トップレベルに `start` / `status` / `resume` / `finish` などの動詞は置かない。
必ず `ai work ...` 配下にまとめる。

### 5.1 最小実装対象

今回の実装対象は以下。

```bash
ai work
ai work start <goal>
ai work status
ai work list
ai work switch <work-id>
ai work push <goal>
ai work pop
ai work defer <text>
ai work idea <text>
ai work note <text>
ai work decide <text>
ai work focus <text>
ai work finish
```

### 5.2 将来拡張候補

今回必須ではないが、将来追加しやすいように設計する。

```bash
ai work inbox
ai work backlog
ai work resume
ai work abandon
ai work rename
ai work show <work-id>
ai work remove <work-id>
```

## 6. 各コマンド仕様

### 6.1 `ai work`

引数なしで実行した場合、現在の work dashboard を表示する。

表示内容:

* active work
* focus
* stack
* recent decisions
* ideas
* deferred items
* よく使うコマンド案内

active work がない場合は、開始方法を表示する。

例:

```text
No active work.

Start a new work:
  ai work start "replay履歴をLLMから参照できるようにする"

Useful commands:
  ai work list
  ai work defer "後で検討したいこと"
```

### 6.2 `ai work start <goal>`

新しい work を開始する。

動作:

1. 新しい WorkItem を作成する
2. その WorkItem を active にする
3. 既存 active work がある場合は paused にする
4. 既存の `context new` / `goal set` / `now set` 相当の更新を行う
5. focus は未指定なら goal と同じ、または空でよい

例:

```bash
ai work start "Contextual memoryの運用導線を設計する"
```

出力例:

```text
Started work #42:
  Contextual memoryの運用導線を設計する

Active work is now #42.
```

### 6.3 `ai work status`

現在の作業状態を表示する。

表示内容:

```text
Active work:
  #42 Contextual memoryの運用導線を設計する

Focus:
  複数作業・脱線・後回しの扱いを決める

Stack:
  #43 ai work配下のサブコマンド体系を整理する

Decisions:
  - トップレベルに start/status/finish は置かない
  - ai work 配下にまとめる

Ideas:
  - work は context の高レベルUXとして扱う

Deferred:
  - ブラウザ拡張から同一work contextを参照する

Suggested next:
  ai work focus <text>
  ai work push <goal>
  ai work defer <text>
```

### 6.4 `ai work list`

work 一覧を表示する。

最低限、以下に分類する。

* active
* paused
* deferred
* done

可能なら stack 上の work も分かるようにする。

例:

```text
Active:
  #42 Contextual memoryの運用導線を設計する

Paused:
  #37 replay履歴をLLMから参照できるようにする

Deferred:
  #44 ブラウザ拡張からaibeの同一work contextを参照する

Done:
  #31 AI_EDITOR対応を実装する
```

### 6.5 `ai work switch <work-id>`

active work を切り替える。

動作:

1. 現在の active work を paused にする
2. 指定 work を active にする
3. 指定 work の goal/focus/decision 等を現在 context に反映する

例:

```bash
ai work switch 37
```

出力例:

```text
Switched active work:
  #37 replay履歴をLLMから参照できるようにする
```

### 6.6 `ai work push <goal>`

現在の作業を一時中断し、派生作業を開始する。

用途:

* 本線から一時的に脱線する
* エラー調査を行う
* 設計中に命名だけ別途整理する
* すぐ戻る前提の小作業を行う

動作:

1. 現在 active work を stack に積む
2. 新しい WorkItem を作成する
3. parent_id に元の active work id を設定する
4. 新しい work を active にする

例:

```bash
ai work push "ai work配下のサブコマンド体系を整理する"
```

出力例:

```text
Pushed current work #42 to stack.

Started child work #43:
  ai work配下のサブコマンド体系を整理する
```

### 6.7 `ai work pop`

現在の派生作業を閉じて、stack から前の作業へ戻る。

動作:

1. 現在 active work を paused または done にする
2. 可能であれば現在 work の summary を作る
3. stack から直前の work を取り出し active に戻す
4. child work の decisions を親 work に反映するかどうかは、最初は自動反映しない
5. 出力で「親に反映すべき内容」を提案するだけでもよい

MVP では LLM 要約は不要。
まずは手動入力済みの decisions / notes / ideas を表示するだけでよい。

例:

```bash
ai work pop
```

出力例:

```text
Closed child work #43:
  ai work配下のサブコマンド体系を整理する

Returned to work #42:
  Contextual memoryの運用導線を設計する

Child decisions:
  - トップレベルに start/status/finish は置かない
  - ai work 配下にまとめる
```

stack が空の場合:

```text
Work stack is empty. No previous work to return to.
```

### 6.8 `ai work defer <text>`

現在はやらない作業候補を deferred/backlog に入れる。
active work は変更しない。

例:

```bash
ai work defer "ブラウザ拡張からaibeの同一work contextを参照する"
```

出力例:

```text
Deferred:
  #44 ブラウザ拡張からaibeの同一work contextを参照する

Active work remains:
  #42 Contextual memoryの運用導線を設計する
```

`defer` は `idea` と区別する。

* `idea`: 現在 work に関係する未確定案
* `defer`: 現在 work から外れる後回し作業

### 6.9 `ai work idea <text>`

現在の active work に idea を追加する。

既存の `ai idea add` 相当。

例:

```bash
ai work idea "push/pop は技術者向けには自然だが、説明文が必要"
```

### 6.10 `ai work note <text>`

現在の active work に note を追加する。

note は decision ではない一般メモ。

例:

```bash
ai work note "Contextual memory は記憶ではなく作業継続として見た方がよい"
```

### 6.11 `ai work decide <text>`

現在の active work に decision を追加する。

例:

```bash
ai work decide "トップレベルに start/status/finish は置かず、ai work 配下にまとめる"
```

### 6.12 `ai work focus <text>`

現在の active work の focus を更新する。

既存の `ai now set` 相当。

例:

```bash
ai work focus "複数作業・脱線・後回しの扱いを決める"
```

### 6.13 `ai work finish`

現在の active work を完了状態にする。

動作:

1. 現在 active work を done にする
2. stack が空でなければ警告する
3. active work を unset する、または stack から復帰するか確認する
4. MVP では確認プロンプトなしで、安全側の挙動にする

推奨挙動:

* stack が空なら active を unset
* stack が空でなければ「先に pop してください」と表示して失敗

例:

```text
Cannot finish work #43 because work stack is not empty.
Use:
  ai work pop
```

または、active が root work の場合:

```text
Finished work #42:
  Contextual memoryの運用導線を設計する
```

## 7. データモデル

Work は複数クライアントから同じ状態を参照できる必要があるため、`aibe` を source of truth とする。`ai` に永続状態は持たせない。

既存 `MemoryOperationDto` だけでは `Paused / Deferred / Done`、`parent_id`、stack、複合状態遷移を原子的に表現できない。そのため `aibe-protocol` に専用の `WorkApply` / `WorkQuery` RPC を追加し、`aibe` の Contextual Memory Pack 内で処理する。

Work state は memory space 単位で次に保存する。

```text
$AIBE_ROOT/memory/spaces/<memory_space_id>/work-state.json
```

既存 memory space の解決、directory、permission、lock を再利用する。mutation は space lock 内で最新 state に適用し、temp file + rename で原子的に置換する。

概念モデルは以下とする。

```rust
struct WorkItem {
    id: WorkId,
    title: String,
    goal: String,
    status: WorkStatus,
    parent_id: Option<WorkId>,
    created_at: DateTime,
    updated_at: DateTime,
    finished_at: Option<DateTime>,
    focus: Option<String>,
    summary: Option<String>,
}

enum WorkStatus {
    Active,
    Paused,
    Deferred,
    Done,
    Abandoned,
}
```

関連する記録:

```rust
struct WorkEntry {
    id: EntryId,
    work_id: WorkId,
    kind: WorkEntryKind,
    text: String,
    created_at: DateTime,
}

enum WorkEntryKind {
    Note,
    Idea,
    Decision,
}
```

永続 state は次の形とする。

```rust
struct WorkState {
    schema_version: u32,
    revision: u64,
    next_work_id: u64,
    active_work_id: Option<WorkId>,
    stack: Vec<WorkId>,
    works: Vec<WorkItem>,
    entries: Vec<WorkEntry>,
}
```

Work ID は space lock 内で単調増加させ、再利用しない。壊れた JSON、未知の schema version、state invariant 違反は明示エラーとし、既存 state を上書きしない。

## 8. 既存機能との対応

`ai work` は既存の低レベル Contextual Memory と同じ memory space / Pack / TurnHook を利用する高レベル UX レイヤーである。

対応関係:

```text
ai work start
  = active work 作成 + goal/focus の保持

ai work focus
  = active work の now 相当を更新

ai work idea
  = active work に idea 相当を追加

ai work note
  = active work に note 相当を追加

ai work decide
  = active work に decision 相当を追加

ai work status
  = goal/now/idea/decision/note/deferred の集約表示

ai work switch
  = active work の切替

ai work defer
  = deferred status の WorkItem 作成
```

既存の `ai goal` / `ai now` / `ai idea` / `ai mem` / `ai context` は削除しない。
ただしユーザー向けの主要導線は `ai work` に寄せる。

Work state を複数の汎用 `MemoryApply` に二重書きしない。複数 RPC の途中失敗と、work 切替時の decision 再活性化を安全に扱えないためである。Work の goal / focus / entries は Work state を正本とし、既存低レベル API の entries は generic memory として独立に維持する。両者は通常 turn の Contextual Memory 解決時に統合する。

## 9. 通常 `ai ...` 実行時の Context 注入方針

active work が存在する場合、通常の `ai "..."` 実行時に以下を薄く注入する。

注入責務は `aibe` の `ContextualMemoryPack` に置く。`ai` の `RequestContext.system_instruction` へ Work 文脈を追加せず、`ask / chat / retry / rerun` を同じ TurnHook 経路で処理する。

必須:

* active work goal
* focus
* recent decisions

任意:

* recent notes
* relevant ideas

注入しすぎないこと。
`idea` や `deferred` を常時大量に入れるとノイズになる。
Work block と既存 generic memory block の合計は、既存 `MEMORY_PROMPT_BUDGET_BYTES` を超えてはならない。

推奨:

```text
注入する:
  goal
  focus
  recent decisions 最大3件
  explicit rules 最大3件

通常は注入しない:
  deferred
  backlog
  古い idea
  大量の note
```

## 10. 脱線検知の将来方針

今回必須ではないが、将来 LLM 応答内で以下のような提案を出せるとよい。

```text
これは現在の作業からの脱線に見えます。どう扱いますか？

1. 現在作業の idea にする
2. 一時的な派生作業として push する
3. 後で見る項目として defer する
4. 別 work に switch する
```

ただし今回の実装では、LLM に自動判断させなくてよい。
まずはユーザーが明示的に `ai work push/defer/idea` できることを優先する。

## 11. CLI 補完・ヘルプ表示

`ai work --help` で各コマンドの意味が明確に分かるようにする。

特に `push` / `pop` / `defer` は説明文が重要。

例:

```text
Commands:
  start   Start a new work context
  status  Show the current work context
  list    List active, paused, deferred, and done works
  switch  Switch active work
  push    Start a temporary child work and stack the current work
  pop     Close current child work and return to previous work
  defer   Save an off-topic idea/work for later without changing active work
  idea    Add an idea to the current work
  note    Add a note to the current work
  decide  Add a decision to the current work
  focus   Update the current focus
  finish  Finish the current work
```

日本語説明を持てる構造なら、以下のニュアンスにする。

```text
push   現在の作業を一時退避して、派生作業を始める
pop    派生作業を閉じて、元の作業に戻る
defer  現在の作業から外れる思いつきを後回しにする
```

## 12. エラーケース

### active work がない状態で `idea/note/decide/focus/pop/finish`

エラーにする。

例:

```text
No active work.

Start one:
  ai work start "..."
```

### `pop` したが stack が空

エラーにする。

```text
Work stack is empty. No previous work to return to.
```

### 存在しない work id に `switch`

エラーにする。

```text
Work #99 not found.
```

### done work に `switch`

原則可能にしない。
再開したい場合は将来 `reopen` を追加する。

MVPでは以下でよい。

```text
Work #31 is already done. Reopen is not supported yet.
```

### active work がある状態で `start`

既存 active を paused にして新しい work を active にする。
ただし出力で明示する。

```text
Paused previous work #42.
Started work #45.
```

## 13. テスト方針

最低限、以下のテストを追加する。

### 13.1 コマンドパース

* `ai work`
* `ai work start <goal>`
* `ai work status`
* `ai work list`
* `ai work switch <id>`
* `ai work push <goal>`
* `ai work pop`
* `ai work defer <text>`
* `ai work idea <text>`
* `ai work note <text>`
* `ai work decide <text>`
* `ai work focus <text>`
* `ai work finish`

### 13.2 状態遷移

* `start` で active work が作られる
* active がある状態で `start` すると旧 active が paused になる
* `push` で旧 active が stack に入り、新 work が active になる
* `pop` で前 work に戻る
* `defer` で active work が変わらない
* `switch` で active work が切り替わる
* `finish` で active work が done になる

### 13.3 エラー

* active なしで `idea` はエラー
* active なしで `focus` はエラー
* stack 空で `pop` はエラー
* 存在しない work id への `switch` はエラー
* done work への `switch` はエラー

### 13.4 表示

* `ai work status` に active/focus/decisions/ideas/deferred が出る
* `ai work list` に active/paused/deferred/done が分類表示される
* `ai work` 引数なしで dashboard が出る

## 14. 実装順序

推奨実装順序:

1. `ai work` サブコマンドの clap 定義を追加
2. WorkItem / WorkEntry / WorkState のドメインモデルを追加
3. 永続化層を追加、または既存 Contextual Memory store に work kind を追加
4. `start/status/list` を実装
5. `idea/note/decide/focus` を実装
6. `defer` を実装
7. `switch` を実装
8. `push/pop` を実装
9. `finish` を実装
10. 通常 `ai ...` 実行時の active work 注入を接続
11. テスト追加
12. docs/manual に `ai work` の使い方を追加

## 15. ドキュメントに載せる使用例

```bash
ai work start "Contextual memoryの運用導線を設計する"

ai work focus "複数作業・脱線・後回しの扱いを決める"

ai work idea "idea と defer は分けた方がよい"

ai work decide "トップレベルに start/status/finish は置かず ai work 配下にまとめる"

ai work push "ai work配下のサブコマンド体系を整理する"

ai work decide "push/pop/defer/switch を導入する"

ai work pop

ai work defer "ブラウザ拡張から同一work contextを参照する"

ai work status

ai work finish
```

## 16. 成功条件

今回の実装が成功したと判断できる条件は以下。

* ユーザーが `goal/now/idea/mem/context` を直接意識しなくても、`ai work` だけで作業文脈を扱える
* 複数作業を `switch` で切り替えられる
* 一時的な寄り道を `push/pop` で扱える
* 思いつきを `defer` で後回しにできる
* `ai work status` を見れば、今どの作業をしていて、何に注目していて、何を決めたか分かる
* 通常の `ai ...` が active work の goal/focus/decision を参照できる
* 既存の Contextual Memory 実装を壊さない

## 17. 設計上の注意

`ai work` はタスク管理ツールではない。
目的は、AISH が現在の作業文脈を持ち、ユーザーが脱線しても戻れるようにすることである。

したがって、最初から高機能化しない。

避けるべきこと:

* 優先度、期限、タグ、担当者などを最初から入れる
* work graph を複雑にする
* LLM による自動分類を最初から必須にする
* `ai start` のような文脈不明なトップレベル動詞を増やす
* `idea` を何でも入れるゴミ箱にする
* `defer` と `idea` を混同する

今回の中心は以下である。

```text
現在作業を見る
作業を始める
作業を切り替える
寄り道する
元に戻る
思いつきを後回しにする
決定事項を残す
```

この最小導線を安定させる。

## 18. パック構成の適用

**部分適用**。新しい独立 Pack は作らず、既存 Contextual Memory Pack の client-side / server-side 境界を拡張する。memory enabled 時は `ContextualMemoryPack` が Work RPC、WorkStore、Work injection を提供し、runtime disabled 時は `BasicPack` が Work RPC を既存 memory-disabled error で拒否して injection を no-op にする。`ai --no-default-features` では既存 memory stub 経由で Work CLI を fail-closed にし、composition root は既存 memory Pack 選択箇所の 1 か所を維持する。

## 19. `ai work` と低レベル memory 操作の関係

`ai work` は `WorkState` / `work-state.json` を使う **first-class 作業文脈レイヤー** である。`ai goal` / `ai now` / `ai idea` / `ai mem` / `ai context` は低レベルの Contextual Memory 操作として残す。

両者は無理な双方向同期を行わない。責務が異なる（work = 作業面、memory = 文脈メモリ）ため、どちらが正か曖昧になる二重書き込みを避ける。

通常の `ai ...` turn では、active work block と既存 contextual memory block が必要に応じて両方注入される。

## 20. 将来の multi-client 連携（設計メモ）

将来的にエディタ・ブラウザ・CLI など複数クライアントが同一 aibe に接続し、同じ work context を共有する想定がある。

**今回の実装範囲外** だが、以下を将来方針として記録する。

- `WorkState` の変更は将来 `WorkChanged` event として通知対象になる
- 既存の `MemoryChanged` / `MemorySubscribe` に含めるか、独立した `WorkSubscribe` を作るかは **未決**
- multi-client UX では **active work が変わったときの通知** が重要
- 複数クライアントが同時に work state を更新する場合、競合解決方針が必要（楽観ロック revision、last-write-wins 等）
- 当面は単一 aibe 内の永続化共有に留める（リアルタイム購読は未実装）

通知対象の mutation 例: `start`, `focus`, `decide`, `defer`, `push`, `pop`, `finish`, `switch`。
