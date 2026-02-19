# システムプロンプト（sysq）と system.d

`ai` 実行時に、`-S/--system` でシステムインストラクションを指定できますが、指定しない場合はま、**有効な sysq（システムプロンプト）** が結合され、LLM の system instruction として使われます。  
sysq の格納場所・有効/無効の切り替え・優先順位を説明します。

## 格納場所とスコープ

システムプロンプトは **system.d** というディレクトリ単位で管理され、次の 3 つのスコープがあります（優先度は **Project > User > Global**。同じ ID が複数スコープにあれば、プロジェクトの内容が採用されます）。

| スコープ | パス（探索順の目安） |
| --- | --- |
| **Global** | `$AISH_HOME/config/system.d`。`AISH_HOME` 未設定時は `$XDG_CONFIG_HOME/aish/system.d` または `~/.config/aish/system.d` |
| **User** | `~/.aish/system.d` |
| **Project** | カレントディレクトリから親へ遡り、最初に見つかった `.aish/system.d` |

- 各 **system.d** ディレクトリ内に、1 ファイル 1 機能のテキストファイルを置きます。
- **ID** は、拡張子を除いた相対パスで表します（例: `base/general` → `base/general.txt` または `base/general`、`dev/coding` → `dev/coding.txt`）。

## 有効/無効の切り替え

各 system.d ディレクトリ内の **`enabled`** ファイルに、有効にしたい ID を 1 行 1 件で列挙します。

```text
base/general
lang/japanese
dev/coding
dev/shell
```

- 空行や前後の空白は無視されます。
- ここに書かれた ID だけが「有効」とみなされ、結合対象になります。
- 結合時の **並び順** は、Global の enabled の並び → User の enabled の並び → Project の enabled の並びです。同じ ID が複数スコープにあれば、内容は Project で上書きされますが、出現順は global → user → project の有効 ID を重複なしで並べた順になります。

## aish での管理コマンド

`aish` から sysq の一覧・有効/無効を操作できます。

- **`aish sysq list`**  
  各スコープで利用可能なシステムプロンプトと、有効状態を一覧表示します。
- **`aish sysq enable <id> [id...]`**  
  指定した ID を、該当スコープの `enabled` に追加します。
- **`aish sysq disable <id> [id...]`**  
  指定した ID を、該当スコープの `enabled` から削除します。

操作対象のスコープは、カレントディレクトリや `AISH_HOME` などに応じて決まります（プロジェクトの `.aish/system.d` がある場合はプロジェクトスコープが使われます）。

## ai 実行時の挙動

- **`ai -S "..."` を指定した場合**  
  指定した文字列がそのまま system instruction として使われ、sysq は参照されません。
- **`-S` を省略した場合**  
  上記のスコープと `enabled` に従って解決されたシステムプロンプトが結合され、区切りとして `\n\n---\n\n` が挟まれた 1 本の文字列が system instruction として渡されます。  
  有効な sysq が 1 つもない場合は、system instruction は送らずに LLM を呼び出します。

## 設定例

- グローバルに「日本語で答える」「コードは簡潔に」などを入れた `base/general.txt` を用意し、`enabled` に `base/general` を追加する。
- プロジェクトごとに `.aish/system.d` を作り、そのプロジェクト専用のルール（例: 「このリポジトリは Rust 1.80 以上」）を別 ID で追加し、`enabled` に並べる。

詳細なパスやマージ順は、実装では `core/common/src/system_prompt.rs` の `merge_enabled_ordered` および `core/ai/src/adapter/resolve_system_instruction.rs` を参照してください。

## 提案コマンド（PromptReady マーカー・Alt+S 注入）

`ai` の `queue_shell_suggestion` ツールでキューしたコマンドは **自動では注入されません**。提案がある場合、aish はプロンプト表示完了（PromptReady マーカー検知）時に **ベルを鳴らし**、プロンプトの `(aish:...)` 部分に **提案コマンドの先頭5文字 + `..`**（例: `git co..`）を表示します。ユーザーが **Alt+S** を押したタイミングで、そのコマンドが未実行の 1 行としてシェルに注入されます。

- **マーカー**: OSC シーケンス `ESC ] 999 ; aish-prompt-ready BEL`（`\x1b]999;aish-prompt-ready\x07`）
- **設定**: `aish` が `--rcfile` で読み込む **.aishrc**（`$AISH_HOME/config/aishrc`）のプロンプト末尾にこのマーカーを埋め込み、`(aish:...)` には `__aish_prompt_label`（提案あり時は `prompt_suggestion.txt` の内容、通常時は part サイズ）を使います。
  - **bash**: 既定の aishrc を参照。
  - **zsh**: 同様に `__aish_prompt_label` とマーカーを組み合わせてください。
- **注入**: **Alt+S**（`\x1b s`）でキューされたコマンドをそのまま注入。blocked の概念は廃止されています。

マーカーを入れていない環境では、提案時のベル・プロンプト表示・Alt+S 注入は期待どおりに動作しません。既存の SIGUSR1 フック（ロールオーバー等）はそのままでよく、PromptReady 検知はこのマーカー経由のみです。
