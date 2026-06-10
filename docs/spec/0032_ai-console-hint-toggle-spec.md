# 0032 — `ai` コンソールヒント切り替え 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定  
> **起票**: 2026-06-10  
> **関連**: [architecture.md](../architecture.md)、[testing.md](../testing.md)、[ai.config.example.toml](../ai.config.example.toml)、[0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)

## 目的

`ai` が TTY 時に付与している console 向け system instruction を、turn 単位で明示的に on/off できるようにする。

本書で確定するのは次の 3 点である。

1. CLI で turn 単位に切り替えできること
2. `~/.config/ai/config.toml` の `[ask]` でデフォルトを指定できること
3. `quiet` / `log_tail_bytes` と同じ優先順位モデルで、preset も含めて解決すること

## 非目標

- `aibe` のプロトコル変更
- `aish` の挙動変更
- `aibe_client` へ CLI / config の読み取り責務を持ち込むこと
- `--format` 時や非 TTY 時に console hint を強制的に有効化すること
- console hint の文言そのものの再設計

## 現状

`ai` は現状、TTY かつ `--format` 未指定のときに端末サイズから console 用 system instruction を生成し、`context.system_instruction` として aibe に送る。

また、`--format` 指定時と非 TTY 時は送らない。これは `docs/architecture.md` の既存方針と一致している。

実装の入口は主に次の 2 箇所である。

- `ai/src/main.rs` の `request_from_messages`
- `ai/src/adapters/outbound/aibe_client.rs` の `AibeUnixClient::to_client_request`

## 決定事項

### 1. CLI オプション

console hint の turn 単位切り替えは次の組にする。

- `--console-hint` / `-H`
- `--no-console-hint` / `-N`

`-H` は enable、`-N` は disable（No hint）を表す。`-H` と `-N` は同時指定不可。

理由:

- `-q` は既存で `quiet` に使われているため衝突させない
- `-h` は help に予約されているため使わない
- `H` / `N` は hint の on/off を対にし、既存 TurnOptions の short と衝突しない

### 2. 設定ファイル

`~/.config/ai/config.toml` の `[ask]` に次を追加する。

- `console_hints = true|false`

デフォルトは **true** とする。つまり、config 未設定時の hardcoded default は現行互換の「TTY 時は有効」である。

### 3. preset

`[presets.*]` も `console_hints` を持てるようにする。

これは `quiet` / `log_tail_bytes` と同じく、preset が turn 既定を上書きできるようにするためである。

### 4. 優先順位

解決順は次とする。

1. CLI 明示値
2. preset `[presets.*].console_hints`
3. `[ask].console_hints`
4. hardcoded default `true`

CLI 明示値は `--console-hint` / `--no-console-hint` を指す。

### 5. 付与条件

console hint の最終付与条件は次の論理積とする。

- `console_hints_effective = requested && tty && output_format.is_none()`

ここでの `requested` は上記優先順位で解決した on/off である。

したがって次は常に無効化される。

- 非 TTY
- `--format json|tsv|env`

このときは「ユーザーが要求したが、eligibility により抑制された」という扱いにし、エラーにはしない。

### 6. 対象 subcommand

対象は `TurnOptions` を持つ次の subcommand に限定する。

- `ask`
- `chat`
- `retry`
- `rerun`

`status` / `doctor` / `ping` / `history` / `complete` は対象外とする。

### 7. `aibe_client` 経路のスコープ

この機能は **`ai` の turn 解決機能**として定義し、`aibe_client` 自体に新しい設定読み取り責務は持たせない。

つまり、policy の正本は `ai` の composition root / turn 解決であり、`aibe_client` は既に解決済みの `RequestContextInput` を serialise するだけに寄せる。

`aibe_client` 側で `detect_terminal_size()` を直接呼んで console hint を決めてはいけない。端末サイズと `--format` を見て `system_instruction` を決めるのは、`ai` 側の turn 解決と `console_context` に閉じる。

`request_from_messages` と `AibeUnixClient::to_client_request` の両方があるため、実装ではこの 2 つが同じ resolved policy を使うことを必須とする。

この方針により、今後 `aibe_client` を再利用する別クライアントが出ても、console hint の有無を policy として再定義しなくてよい。

### 8. dry-run 表示

`--dry-run` には console hint の解決結果を反映する。

出力には少なくとも次の 5 要素を含める。`requested` は TTY / format を加味する前の resolved boolean を指す。

- `console_hint.requested` : `true|false`
- `console_hint.source` : `cli|preset|config|default`
- `console_hint.tty` : `true|false`
- `console_hint.output_format` : `json|tsv|env|none`
- `console_hint.effective` : `true|false`

`console_hint.effective` が `false` の場合は、抑止理由も 1 つだけ出す。

- `console_hint.suppressed_by=tty`
- `console_hint.suppressed_by=format`
- `console_hint.suppressed_by=none`

ただし、実際に生成された system instruction 本文は出さない。

## 仕様

### 1. CLI の意味論

`--console-hint` と `--no-console-hint` は互いに排他的な boolean 指定とする。

- `--console-hint` はその turn だけ console hint を有効にする
- `--no-console-hint` はその turn だけ console hint を無効にする
- `-H` は `--console-hint`、`-N` は `--no-console-hint` の short form とする

既存の `TurnOptions` にある他の flag と同じく、turn 単位でのみ解釈する。

### 2. config / preset の意味論

`[ask].console_hints` は `ai` の既定動作を設定する。

`[presets.*].console_hints` は preset 適用時に `[ask]` を上書きする。

この値は「TTY かどうか」や「`--format` の有無」を判断しない。あくまで「有効化したいか」を表すだけである。

### 3. TTY / format との合成

console hint は presentation を整えるための system instruction であり、機械可読出力と両立しない。

したがって次の順で評価する。

1. resolved `console_hints`
2. TTY 判定
3. `--format` 判定

`--format` が存在する場合は常に抑止し、`TTY` でも付けない。

### 4. 影響範囲

この機能は `ask` / `chat` / `retry` / `rerun` の request 組み立てにのみ影響する。

`status` / `doctor` / `ping` のような情報表示系には影響しない。

また、`aibe` の wire schema に新しい field は追加しない。

### 5. 実装配置の原則

policy は `ai` 側で決め、`RequestContextInput` へ落とす。

- `RequestContextInput` は `system_instruction` を保持するだけ
- `console_context` は「付けるかどうか」の条件判定だけを担う
- `aibe_client` は policy を持たない

この切り分けにより、turn 単位の on/off を `ai` のみで完結できる。

## 受け入れ条件

### unit

- `[ask].console_hints` が config から読める
- `[presets.*].console_hints` が読める
- CLI > preset > config > default の優先順位が固定される
- TTY かつ `--format` なしのときだけ effective true になる
- 非 TTY または `--format` 指定時は effective false になる

### integration

- `ai ask --console-hint` が TTY では console hint を送る
- `ai ask --no-console-hint` が TTY でも console hint を送らない
- `ai chat` / `retry` / `rerun` でも同じ toggle が効く
- `--dry-run` に console hint の解決結果が表示される

### docs

- `docs/0000_spec-index.md` に 0032 を追加する
- `docs/ai.config.example.toml` は実装時に `[ask].console_hints` と `[presets.*].console_hints` の例を追加する

## 補足

`console_hints` を preset 対応にしたのは、`quiet` / `log_tail_bytes` と同じ turn policy として扱うためである。

一方で、`aibe_client` を新たな policy の受け皿にしないことを明確にした。これは transport 層の責務を増やしすぎないためであり、後続の `ai` 以外のクライアントにも同じ解決規則を押し付けないためでもある。
