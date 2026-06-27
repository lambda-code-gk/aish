#!/usr/bin/env bash
# Codex Linux sandbox の安全なバックエンドを選ぶ。呼び出し元から source する。

codex_select_linux_sandbox() {
  local codex="$1"

  CODEX_SANDBOX_ARGS=()
  CODEX_SANDBOX_BACKEND=""

  if "$codex" sandbox -- /bin/true >/dev/null 2>&1; then
    CODEX_SANDBOX_BACKEND="bwrap"
    return 0
  fi

  if "$codex" sandbox --enable use_legacy_landlock -- /bin/true >/dev/null 2>&1; then
    CODEX_SANDBOX_ARGS=(--enable use_legacy_landlock)
    CODEX_SANDBOX_BACKEND="landlock"
    return 0
  fi

  echo "Codex sandbox error: bwrap と Landlock の両方が利用できません。" >&2
  echo "./scripts/codex-fix-linux-sandbox.sh で診断してください。" >&2
  return 1
}
