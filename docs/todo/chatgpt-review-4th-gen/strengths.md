# 良い点

[← 索引](README.md)

- **aish がネットワークを持たない** 境界（PTY/子プロセスでログ追記のみ）。
- aibe の Unix socket は `umask 077` と `chmod 600` で作成（ローカルソケットの権限意識）。
- `read_file`: `allowed_roots` と `context.cwd` 基準、canonicalize 後に許可ルート外を拒否。
- `shell_exec`: `allowed_commands` 許可制、subprocess cwd はクライアント `context.cwd`。

以前の AISH より **事故りにくい設計** になっている、という評価。
