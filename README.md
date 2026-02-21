# AISH

AISH は、LLM によって Linux の CUI 体験を強化するための **CUI 自動化フレームワーク**です。  
Rust 製の `ai` / `aish` バイナリとして提供され、ターミナルの入出力をコンテキストとして LLM に渡し、

- シェルに対する操作や質問を **自然言語で記述**したり
- エージェントに **コマンド実行・ファイル編集・検索などを自律的に任せたり**
- **レビューやコミットメッセージ生成などの開発タスクを自動化**したり

といったことができます。

⚠️ **Important**: `ai` / `aish` はターミナルの入出力やファイル内容を外部の LLM API（例: OpenAI, Google など）に送信します。  
機密情報・大きなバイナリ・個人情報などを含むデータは送らないでください。自己責任で利用してください。

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/lambda-code-gk/aish)

## ✨ Features

* **Context-aware interactions**: `aish` がターミナルの入出力を記録し、`ai` コマンドがそのコンテキストを踏まえて応答できます。
* **AI Agent**: ツール使用により簡易的なエージェントとして振る舞えます。
* **Memory / Session System**: セッションログ・Part 履歴を Rust の usecase / adapter で管理し、LLM コンテキストやツールから参照します。
* **Security Scanning**: `tools/leakscan` による高速な秘密情報検出エンジン（キーワード・正規表現・エントロピー）を統合可能です。
* **Task-oriented workflows**: `ai <task>` でタスクスクリプトを実行。タスクは `task.d` に配置し、`ai --list-tasks` で一覧できます。
* **TAB 補完**: `ai --generate bash` / `zsh` / `fish` で補完スクリプトを生成し、シェルに読み込むと、タスク名・`-p` プロファイル・`-M` モードなどを TAB キーで補完できます。

## 📚 Documentation

- **はじめに**: [docs/README.md](docs/README.md) — ドキュメントの目次と読む順番
- 詳細なコンセプトとユースケース: [docs/overview.md](docs/overview.md)
- コマンド一覧と概要: [docs/commands.md](docs/commands.md)
- `aish` の使い方とセッション管理: [docs/aish-usage.md](docs/aish-usage.md)
- `ai` の使い方（最も詳しいガイド）: [docs/ai-usage.md](docs/ai-usage.md)
- セキュリティ・プライバシーと leakscan: [docs/security.md](docs/security.md)
- よくある質問とトラブルシューティング: [docs/faq.md](docs/faq.md)

## 🚀 Quick Start

### Requirements

- **OS**: Linux
- **Shell**: bash（その他のシェルでも利用可能ですが、`aishrc` の挙動は bash 前提です）
- **Rust & Cargo**: コアツールおよび `ai` / `aish` をビルドするために必須
- **補助コマンド**: `rg`（ripgrep）など、一部のテスト・ビルドスクリプトで利用

### Installation

1. **リポジトリをクローン**:

    ```bash
    git clone https://github.com/lambda-code-gk/aish.git
    cd aish
    ```

2. **コアツールとバイナリをビルド**:

    プロジェクトルートで:

    ```bash
    ./build.sh          # リリースビルド
    # または
    ./build.sh --debug  # デバッグビルド
    ```

    ビルド成果物は **`dist/bin/`** に配置されます（リポジトリは汚れません）。

3. **開発時の実行環境（推奨）**:

    実行時データ（設定・セッション・ログ）をリポジトリ直下に作らず、`.sandbox/xdg/` に隔離して使う場合:

    ```bash
    source scripts/dev/env.sh   # XDG_* と PATH を設定
    ./build.sh --debug
    aish init                   # 必要なら初期設定を展開（AISH_DEFAULTS_DIR は env.sh で設定済み）
    aish                        # セッション開始
    ```

    通常利用（インストール後）では、設定・セッションは **XDG ベース**（`~/.config/aish`, `~/.local/state/aish` 等）に保存されます。  
    **AISH_HOME** を指定した場合のみ、その1ディレクトリ配下に完結します（ポータブル/開発用）。

4. **LLM API キーの設定**:

    利用する LLM プロバイダの API キーを環境変数として設定します（例）:

    ```bash
    export OPENAI_API_KEY=sk-...
    export GOOGLE_API_KEY=...
    ```

    実際にどの変数を参照するかは、利用するドライバや環境により異なります。

### Launching AISH

新しい AISH セッションを開始するには:

```bash
$ aish
(aish:0)$
```

プロンプトは `(aish:N)$` の形式になり、`N` はセッションコンテキストのサイズ（例: `(aish:1.2K)$`）などを表します。

### Session Management

`ai` / `aish` では、シェル上で実行されたコマンドを記録し、会話履歴としてLLMのAPIに送信します。
セッションの毎にディレクトリが用意され、その中で作業ファイルが展開されます。

現在、セッションディレクトリ内では以下の作業が行われています。

- コンソールの標準入出力のキャプチャ
- leakscanによる機密情報の検知
- 履歴のインデックス情報の作成
- ツールからの参照

セッションの実体は、**AISH_HOME 指定時**は `$AISH_HOME/state/session/<id>/`、**未指定時**は `$XDG_STATE_HOME/aish/session/<id>/`（例: `~/.local/state/aish/session/<id>/`）に置かれます。
`<id>` は日時を元に生成されるユニークな名前が自動的に振られます。

`aish -s "<session_dir>"` のようにパスを指定すると、セッションを再開できます。

## 🛠 Available Tasks

`ai <task> [message...]` 形式で、タスクスクリプトを実行できます。タスクは次のパス（先に存在する方）から検索されます。

- `$AISH_HOME/config/task.d/`
- `$XDG_CONFIG_HOME/aish/task.d/`（例: `~/.config/aish/task.d/`）

| 例 | 説明 |
| :--- | :--- |
| `ai commit_staged` | ステージ済み変更からコミットメッセージを生成しコミットを行う |
| `ai <task> ...` | 任意のタスク名とメッセージ。タスクが存在しない場合は LLM への通常問い合わせとして扱われます。 |

利用可能なタスク一覧は `ai --list-tasks`、オプションは `ai --help` で確認してください。

## 🧰 Support Tools

補助ツールは `tools/` 以下にあります。`build.sh` 実行時に **leakscan** がビルドされ、`dist/bin/` に配置されます。

* **`leakscan`**: 秘密情報の誤送信を防ぐための検査エンジン。キーワード・正規表現・Shannon エントロピー等でログやファイルをスキャンします。

その他、`tools/aish-capture` / `aish-render` / `aish-script` 等のサブプロジェクトは存在しますが、現状の `build.sh` では旧実装で使用していたもので今はビルド対象外です（必要に応じて個別にビルド可能）。

## 📂 Project Structure

現在の Rust ベース実装の構成は、おおよそ次のようになっています。

```text
aish/
├── core/                 # Rust クレート郡
│   ├── common/           # ai / aish 共通ドメイン・ポート・LLM ドライバ等
│   ├── ai/               # 'ai' コマンド本体
│   └── aish/             # 'aish' コマンド本体
├── assets/defaults/      # 初期設定テンプレ（aish init で XDG/AISH_HOME に展開）
├── dist/                 # ビルド成果物（dist/bin/。.gitignore 済み）
├── .sandbox/             # 開発用サンドボックス（scripts/dev/env.sh で使用。.gitignore 済み）
├── tools/                # サブツール（build.sh では leakscan をビルド）
│   ├── leakscan/
│   └── aish-capture/ 等  # サブプロジェクト（必要に応じて個別ビルド）
├── scripts/dev/          # 開発用スクリプト（env.sh, reset.sh）
├── old_impl/             # 旧シェル実装（Bash + Python ベース）
└── tests/                # アーキテクチャ・ユニット・統合テスト（architecture.sh, units.sh, integration.sh）
```

### 開発・テスト

コード変更前後には、ルートで次を実行して既存機能が壊れていないことを確認してください。

```bash
./tests/architecture.sh && ./tests/units.sh && ./tests/integration.sh
```

詳細なアーキテクチャや開発ルールは [AGENTS.md](AGENTS.md) を参照してください。

## 🧭 Roadmap & Future Plans

- **完全な Rust 化**: 旧シェル実装で提供していたタスクやメモリシステムを、Rust の usecase / adapter アーキテクチャに統合。
- **Self-Improvement**: エージェント自身がツールやプロンプト、メモリ構造を改善していく自己改善ループ。
- **Context Optimization**: 長いターミナルログや履歴に対して、自動要約・重要部分抽出などを行うコンテキスト最適化。
- **Visual Understanding**: 将来的な拡張として、ターミナル状態やスクリーンショットを取り込んだ理解。

## 📄 License

This project is licensed under the MIT License. See the `LICENSE` file for details.

