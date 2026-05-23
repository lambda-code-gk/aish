# aish workspace

Rust ワークスペース。3 つのクレートで構成されます。

AI・人間向けの開発規約は [AGENTS.md](AGENTS.md) と [.cursor/rules/](.cursor/rules/) を参照。

設計・テスト・セキュリティ: [docs/architecture.md](docs/architecture.md)、[docs/testing.md](docs/testing.md)、[docs/security.md](docs/security.md)。境界・レイヤーチェック: `./scripts/check-architecture.sh`（`check-hexagonal.sh` を含む）。

| クレート | 種別 | 役割 |
|---------|------|------|
| **aibe** | ライブラリ + バイナリ | 複数 LLM API を呼び出すバックエンド常駐プロセス。クライアントとプロトコルで通信し、バックグラウンドで API 処理を行う |
| **aish** | バイナリ | 起動時にシェルを立ち上げ、入出力・コマンド出力・ファイル I/O をログに記録する |
| **ai** | バイナリ | aish のログをコンテキストにシェル操作を行い、aibe を利用するクライアント |

## ビルド

```bash
cargo build --workspace
cargo test --workspace
```

個別クレート:

```bash
cargo build -p aibe
cargo run -p aibe              # デフォルト: バックグラウンド（デーモン）起動
cargo run -p aibe -- -f        # フォアグラウンド（デバッグ用）
cargo run -p aish
cargo run -p ai
```
