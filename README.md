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
* **AI Agent**: ツール使用により簡易的なエージェントとして振る舞う事ができます。
* **Memory / Session System**: セッションログや履歴を扱う仕組みを Rust のドメイン / usecase として再設計中です。
* **Security Scanning**: `tools/leakscan` による高速な秘密情報検出エンジン（キーワード・正規表現・エントロピー）を統合可能です。
* **Task-oriented workflows**: コードレビュー・コミットメッセージ生成・バグ修正支援など、タスク指向のワークフローを `ai <task>` で呼び出せます。

## 📚 Documentation

- **はじめに**: [docs/README.md](docs/README.md) — ドキュメントの目次と読む順番
- 詳細なコンセプトとユースケース: [docs/overview.md](docs/overview.md)
- コマンド一覧と概要: [docs/commands.md](docs/commands.md)
- `aish` の使い方とセッション管理: [docs/aish-usage.md](docs/aish-usage.md)
- `ai` の使い方（最も詳しいガイド）: [docs/ai-usage.md](docs/ai-usage.md)
- システムプロンプト（sysq）の設定: [docs/system-prompts.md](docs/system-prompts.md)
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

    これにより、以下がビルドされます。

    - `core/common` / `core/ai` / `core/aish`
    - `tools/aish-capture` / `tools/aish-render` / `tools/aish-script` / `tools/leakscan`

    ビルド成果物は `home/bin/` に配置されます。

3. **AISH_HOME と PATH の設定**:

    よく使うシェル（例: `~/.bashrc`）に、次のような設定を追加します。

    ```bash
    # AISH のインストールディレクトリに合わせてパスを書き換えてください
    export AISH_HOME="/path/to/aish/home"
    export PATH="$PATH:$AISH_HOME/bin"
    ```

    リポジトリ直下でビルドしている場合は、開発用として:

    ```bash
    export AISH_HOME="$PWD/home"
    export PATH="$PATH:$AISH_HOME/bin"
    ```

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

セッションの実体は、`AISH_HOME/state/sessions/<id>/` に置かれます。
`<id>`は日時を元に生成されるユニークな名前が自動的に振られます。

`aish -s "$AISH_HOME/state/sessions/<id>"` のようにパスを指定すると、セッションを再開する事ができます。

## 🛠 Available Tasks

`ai <task>` 形式で、さまざまなタスク指向の操作を呼び出せます（一部は旧実装から順次 Rust 化中です）。

| Task | Description |
| :--- | :--- |
| `ai commit_msg` | ステージ済み変更からコミットメッセージを生成します。 |

実際に利用可能なタスクやオプションは、`ai --help` で確認してください。

## 🧰 Support Tools

補助的なコマンドラインツールが `tools/` 以下にまとまっています。`build.sh` 実行時にビルドされ、`home/bin/` に配置されます。

* **`leakscan`**: 秘密情報の誤送信を防ぐための検査エンジン。キーワード・正規表現・Shannon エントロピー等でログやファイルをスキャンします。

## 📂 Project Structure

現在の Rust ベース実装の構成は、おおよそ次のようになっています。

```text
aish/
├── core/                 # Rust クレート郡
│   ├── common/           # ai / aish 共通ドメイン・ポート・LLM ドライバ等
│   ├── ai/               # 'ai' コマンド本体
│   └── aish/             # 'aish' コマンド本体
├── home/                 # 実行時ホーム相当（build.sh で構築）
│   ├── bin/              # ai / aish / aish-capture / leakscan などのバイナリ
│   └── config/           # aishrc, system.d, task.d など
├── tools/                # サブツール
│   └── leakscan/
├── old_impl/             # 旧シェル実装（Bash + Python ベース）
└── tests/                # アーキテクチャ・ユニット・統合テストスクリプト
```

## 🧭 Roadmap & Future Plans

- **完全な Rust 化**: 旧シェル実装で提供していたタスクやメモリシステムを、Rust の usecase / adapter アーキテクチャに統合。
- **Self-Improvement**: エージェント自身がツールやプロンプト、メモリ構造を改善していく自己改善ループ。
- **Context Optimization**: 長いターミナルログや履歴に対して、自動要約・重要部分抽出などを行うコンテキスト最適化。
- **Visual Understanding**: 将来的な拡張として、ターミナル状態やスクリーンショットを取り込んだ理解。

## 📄 License

This project is licensed under the MIT License. See the `LICENSE` file for details.

