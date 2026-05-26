# P1: `aibe-protocol` / `aibe-client` 分離

[← 索引](README.md)

[concerns.md](concerns.md) §3 の長期対応。別フロントエンドを作る前にクライアント境界を固める。

## 現在

```text
ai  →  aibe
aibe が protocol / client / server / agent を全部持つ
```

## 推奨

```text
aibe-protocol  # wire DTO, ErrorCode, ToolName, request/response
aibe-client    # Unix socket client, ping, ensure_running
aibe           # server, LLM, agent loop, tools
ai             # aibe-client + aibe-protocol のみ
```

スプリントへの落とし込み: [sprints.md](sprints.md) Sprint 2
