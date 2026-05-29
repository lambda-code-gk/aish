# P3: ログ連携を AISH らしい価値に

[← 索引](README.md)

現状の価値の芯は、`aish` のシェルログを `ai` が `--log` で読み、aibe へ context として渡せる点。ここを磨くと AISH らしくなる、というレビュー。

## 欲しい機能

- `aish shell` で現在セッション ID
- `ai ask` が現在セッションのログを自動検出（毎回 `--log` 不要）
- ログ tail + 要約済み context
- 直近コマンド・直近エラー・cwd・git 状態の構造化 context

## 目標体験

```bash
aish
ai ask "今のエラーを直す方針を出して"
```

「ただの CLI LLM クライアント」から抜ける、という評価。

正式指示書: [0019_aish-session-log-integration-spec.md](../../done/0019_aish-session-log-integration-spec.md)（**実装済み**）

スプリントへの落とし込み: [sprints.md](sprints.md) Sprint 3
