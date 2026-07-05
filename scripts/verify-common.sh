# verify.sh / verify-targeted.sh 共通: 段階表示・経過時間・進捗ファイル。
# shellcheck shell=bash

verify_progress() {
  local msg="==> $*"
  echo "$msg"
  if [[ -n "${VERIFY_PROGRESS_FILE:-}" ]]; then
    # 別ターミナルで tail -f して待ち時間を可視化する（verify 本体はパイプしない）。
    printf '%s %s\n' "$(date -Iseconds 2>/dev/null || date '+%Y-%m-%dT%H:%M:%S%z')" "$msg" >>"$VERIFY_PROGRESS_FILE"
  fi
}

verify_run() {
  verify_progress "$*"
  local start=$SECONDS
  "$@"
  local elapsed=$((SECONDS - start))
  verify_progress "done (${elapsed}s): $*"
}

verify_init_progress_file() {
  if [[ "${VERIFY_PROGRESS:-0}" == "1" && -z "${VERIFY_PROGRESS_FILE:-}" ]]; then
    VERIFY_PROGRESS_FILE="${ROOT:-.}/.verify-progress"
  fi
  if [[ -n "${VERIFY_PROGRESS_FILE:-}" ]]; then
    : >"$VERIFY_PROGRESS_FILE"
    verify_progress "progress log: $VERIFY_PROGRESS_FILE"
  fi
}

verify_print_total() {
  local label="$1"
  local elapsed=$((SECONDS - VERIFY_START_SEC))
  local mins=$((elapsed / 60))
  local secs=$((elapsed % 60))
  if ((mins > 0)); then
    verify_progress "${label} total: ${mins}m ${secs}s"
  else
    verify_progress "${label} total: ${secs}s"
  fi
}

verify_validate_packages() {
  local packages="$1"
  local pkg
  for pkg in $packages; do
    case "$pkg" in
      aibe | aibe-client | aibe-protocol | aish | aish-replay | ai) ;;
      *)
        echo "verify: unknown VERIFY_PACKAGES entry: $pkg" >&2
        exit 2
        ;;
    esac
  done
}
