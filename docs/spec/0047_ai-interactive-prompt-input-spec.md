# 0047 — `ai` 対話的プロンプト入力モード 設計書

> **種別**: 設計書（`docs/spec/`）  
> **状態**: 設計確定（実装済み）  
> **起票**: 2026-06-21  
> **関連**: [0027_ai-ux-spec.md](0027_ai-ux-spec.md)、[0030_ai-smart-entry-spec.md](0030_ai-smart-entry-spec.md)、[0045_pack-composition-spec.md](0045_pack-composition-spec.md)、[architecture.md](../architecture.md)、[testing.md](../testing.md)、[ai-ux.md](../manual/ai-ux.md)

## 0. 目的

`ai` を引数なしで単体実行し、かつ `stdin` が TTY のときに、対話的プロンプト入力モードを起動する。

この機能の正本は次の 3 点である。

1. 入力方法は `argv`、`stdin pipe`、`対話的プロンプト入力` の 3 系統を維持する
2. 対話的プロンプト入力は **bare `ai` のみ** で起動し、明示的なサブコマンド呼び出しや option-only 呼び出しでは起動しない
3. プロンプト収集、TTY 判定、一時ファイル、外部エディタ起動は ports & adapters で分離する

`ai chat` は既存どおり別の REPL であり、この仕様の対象外である。`ai chat` の `rustyline` 経路は維持し、今回の変更で統合しない。

## 1. 非目標

- `ai` から LLM を直接呼ぶこと
- `ai chat` の REPL をこの仕様で置き換えること
- `aibe` の RPC 形式や wire protocol を変更すること
- `ai` に新しいユーザー向けフラグを追加して対話モードを制御すること
- 対話入力モードを `docs/0000_spec-index.md` に先行登録すること
- Windows 対応

## 2. 現状との差分

### 2.1 入口判定

| 現状 | 0047 後 |
|------|--------|
| `normalize_args` が bare `ai` を常に `ai ask` に正規化する | bare `ai` の provenance を保持し、対話入力モードへ分岐できるようにする |
| `ai ask` と bare `ai` の区別が runtime では消える | bare `ai` と explicit `ai ask` を区別できる |
| `ai --preset ...` のような option-only 起動も `ask` に落ちる | option-only 起動は explicit 扱いのままにし、対話入力モードに入れない |
| `resolve_ask_message` は message が無いと `missing message` で bail する | bare `ai` のみ対話入力モードへ進み、explicit ask は既存の usage error を維持する |

### 2.2 入力ソース

| 現状 | 0047 後 |
|------|--------|
| `argv` / `stdin pipe` / `missing message` の 3 分岐 | `argv` / `stdin pipe` / `対話的プロンプト入力` の 3 分岐 |
| `stdin TTY` の空入力は `missing message` 扱い | `stdin TTY` で bare `ai` なら編集 UI を出し、送信完了まで AI を呼ばない |
| 外部エディタでの下書き経路がない | `AI_EDITOR` / `VISUAL` / `EDITOR` で外部エディタ経路を追加する |
| `ai chat` は `rustyline` のみ | 対話的プロンプト入力は別 adapter として追加する |

### 2.3 テスト

| 現状 | 0047 後 |
|------|--------|
| `normalize_args` と既存 `ask` の E2E が中心 | bare `ai` の TTY 起動、editor precedence、comment 行除去、empty/abnormal exit の拒否を追加する |
| 非 TTY の pipe 経路は既存通り | pipe 経路の回帰を維持する |
| 手動確認は `ai chat` 中心 | bare `ai` のプロンプト入力手順を手動確認に追加する |

## 3. パック構成の適用

**No**。この機能は optional な機能束を core から脱着する話ではなく、`ai` CLI の単一の入力収集経路を増やすだけである。`ai` 本体が扱うのは「いつ対話入力を開始するか」という入口判定と、editor / TTY / 一時ファイルを扱う adapter の選択であり、Active Pack / Basic Pack を持つ独立した optional runtime にはならない。したがって 0045 のパック構成は採用せず、通常の ports & adapters として実装する。

## 4. 仕様

### 4.1 起動条件

対話的プロンプト入力モードは、次の条件をすべて満たすときのみ起動する。

1. `ai` が **bare root invocation** で起動される
2. `stdin` が TTY である
3. その起動が `ask` / `chat` / `retry` / `rerun` / `history` / `status` / `doctor` / `ping` / `complete` / `goal` / `now` / `idea` / `mem` / `context` などの明示的な subcommand 呼び出しではない
4. その起動が option-only 呼び出しでもない
5. 明示的に非対話として扱う経路ではない

ここでいう bare root invocation とは、`argv[1]` が存在しない `ai` のみの起動を指す。`ai ask` は explicit ask であり、bare root invocation ではない。`ai --preset ...` のような option-only 起動も explicit 扱いであり、対話入力モードには入れない。

### 4.2 入口判定の扱い

現行の `normalize_args` は bare `ai` を `ai ask` に正規化しているが、このままでは bare `ai` と `ai ask` の provenance が消える。0047 では、CLI 正規化の段階で次を保持する必要がある。

- `implicit root invocation` か `explicit ask` か
- `option-only` か `message-bearing` か
- `stdin pipe` か `stdin TTY` か

実装上は、`resolve_ask_message` の前段に「入力ソース分類」を置き、bare `ai` にだけ対話入力モードを許可する。`ai ask` に message が無い場合は既存どおり usage error を返す。

### 4.3 入力解決フロー

入力の優先順位は次のとおりとする。

1. 明示ファイル入力
2. 明示的な `stdin` 入力
3. `argv` による message
4. `stdin pipe`
5. bare `ai` のみ対話的プロンプト入力

`stdin pipe` は既存どおり `echo ... | ai` を壊さない。pipe は対話入力モードに優先するため、`stdin` が非 TTY なら prompt UI は起動しない。

### 4.4 エディタ選択

対話的プロンプト入力のエディタ選択は環境変数のみを用いる。設定ファイルは MVP の対象外である。

優先順位は次のとおり。

1. `AI_EDITOR`
2. `VISUAL`
3. `EDITOR`
4. 内蔵ミニエディタ

外部エディタ名は空文字を無視し、次の候補へ進む。複数の環境変数が設定されている場合は先勝ちにする。ここでの選択は CLI フラグでは制御しない。

外部エディタの値は shell-words 相当で 1 回だけ分解し、先頭要素を実行ファイル、残りを引数として扱う。シェル展開や `sh -c` は使わない。分解できない値は無効として次の候補へ進む。

### 4.5 外部エディタ経路

外部エディタ経路は adapter として実装する。ここで責務を分離する。

- 一時ファイルの生成
- エディタプロセスの起動
- `stdin` / `stdout` / `stderr` の取り扱い
- エディタ終了コードの判定
- 編集後テキストの読み戻し

外部エディタに渡すファイルは `.md` 拡張子を持つ一時ファイルとする。下書きにはユーザー向けの説明を含めてもよいが、送信前に除去対象の comment 行を取り除く。

コメント行は Markdown 本文と衝突しない専用のテンプレート注釈に限定する。MVP では `<!-- ai-prompt: ... -->` 形式の HTML comment を使い、送信前にその注釈ブロックだけを除去する。`#` 見出しや本文の Markdown は削除しない。

除去後の内容が空、または空白のみなら、AI を呼ばずに終了する。

外部エディタが次のいずれかに該当する場合も AI を呼ばない。

- 起動失敗
- 非 0 終了
- シグナル終了
- 読み戻し失敗

### 4.6 内蔵ミニエディタ MVP

外部エディタが選ばれない場合、内蔵ミニエディタを使う。MVP の最小要件は次のとおり。

- `Enter` で改行を挿入する
- `↑` / `↓` で上下の行へ移動して編集する
- `Ctrl+Enter` で送信を確定する（Kitty / WezTerm 等のキーボード拡張対応端末）
- `Alt+Enter` でも送信できる（従来端末向けの代替）
- `Ctrl+C` でキャンセルする
- 複数行入力を許可する
- 送信前の空入力は拒否する

内蔵ミニエディタの実装は `reedline` を正本とし、MVP では必ずこの経路を満たす。`reedline` の初期化や端末連携に失敗した場合は、AI を呼ばずに終了する。

### 4.7 送信前検証

入力の最終値は送信前に次を満たす必要がある。

- 1 行でも実体がある
- comment 行除去後に空でない
- 異常終了やキャンセルではない

条件を満たさない場合、`aibe` への接続や履歴記録は行わない。

### 4.8 アーキテクチャ境界

ports & adapters の境界は次のように置く。

| 層 | 責務 |
|----|------|
| **application** | bare root invocation と explicit invocation の分類、prompt mode の起動判断、送信前検証 |
| **ports** | prompt input、editor 起動、temp file、TTY 判定、コメント除去の抽象化 |
| **adapters** | `reedline` ベースの内蔵ミニエディタ、外部エディタ起動、tmpdir / temp file / 端末 API |
| **domain** | 受け取った完成済み message のみを扱う |

この仕様では `ai chat` の行入力 adapter とは共有しない。`chat` は既存の `rustyline` ベース REPL を維持する。

## 5. 受け入れ条件

1. `ai` を bare で TTY 実行したとき、対話的プロンプト入力モードが起動する
2. bare `ai` の prompt で送信した内容が `aibe` に 1 回だけ送られる
3. `echo ... | ai` は既存どおり pipe 入力として処理され、prompt UI を出さない
4. `ai hello` と `ai 'hello'` は既存どおり argv 入力として処理される
5. `ai ask`、`ai --help`、`ai --version`、subcommands、option-only 呼び出しでは prompt UI を出さない
6. `AI_EDITOR` / `VISUAL` / `EDITOR` の優先順位で外部エディタが選ばれる
7. 外部エディタで comment 行を除去した結果が空なら AI を呼ばない
8. 外部エディタが異常終了したら AI を呼ばない
9. 内蔵ミニエディタで `Enter` / `Ctrl+Enter`（または `Alt+Enter`） / `Ctrl+C` の挙動が仕様どおりになる
10. `ai chat` の既存 REPL と `stdin pipe` の既存挙動が回帰しない

## 6. テスト方針

### 6.1 単体テスト

- bare root invocation と explicit ask を区別する分類ロジック
- option-only 呼び出しが対話モードに入らないこと
- editor precedence の解決順
- comment 行除去
- empty input の拒否
- abnormal exit の拒否

### 6.2 統合テスト

- TTY 付きプロセス起動で bare `ai` の prompt UI を確認する
- `stdin pipe` で既存の `resolve_ask_message` 経路を確認する
- `ai ask` の explicit 経路が prompt UI に落ちないことを確認する
- `AI_EDITOR` をダミースクリプトに差し替え、外部エディタ経路を確認する
- すべてのテストは mock aibe / fake provider のみを使い、実 LLM API には触れない

### 6.3 手動検証

TTY と外部エディタを使うため、少なくとも 1 回は manual check を行う。`docs/manual/ai-ux.md` に bare `ai` の検証手順を追加することを前提にする。

### 6.4 ドキュメント更新

実装と同じ変更で、`README.md` と `ai --help` の利用説明を更新する。`ai` の日常導線、bare `ai` の対話入力、`AI_EDITOR` / `VISUAL` / `EDITOR` の優先順位を README と help から辿れるようにする。

## 7. 実装メモ

- `resolve_ask_message` を単純な message 収集関数のままにはしない
- bare root invocation の provenance を失わない
- prompt input は `ai chat` と共有しない
- `docs/0000_spec-index.md` はこの段階では更新しない
