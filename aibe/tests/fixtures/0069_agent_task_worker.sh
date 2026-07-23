#!/bin/sh
set -eu

mode="${1:-success}"
input=$(dd bs=65536 count=1 2>/dev/null)

for required in '"schema_version":1' '"delegation_depth":1' '"objective"' '"completion_criteria"' '"cwd"'; do
  case "$input" in
    *"$required"*) ;;
    *) printf '%s\n' 'invalid envelope' >&2; exit 64 ;;
  esac
done

case "$mode" in
  success)
    printf '%s\n' 'fixture worker ran' > agent-task-output.txt
    printf '%s' '{"schema_version":1,"summary":"fixture completed","status":"done"}'
    ;;
  blocked)
    printf '%s' '{"schema_version":1,"summary":"waiting on external auth","status":"blocked","blockers":["missing API credential"]}'
    ;;
  nonzero)
    printf '%s\n' 'configured failure' >&2
    exit 7
    ;;
  malformed)
    printf '%s' '{not-json'
    ;;
  large)
    i=0
    while [ "$i" -lt 40000 ]; do printf x; i=$((i + 1)); done
    ;;
  timeout)
    (sleep 30; printf '%s\n' survived > agent-task-timeout-sentinel.txt) &
    printf '%s\n' "$!" > agent-task-child.pid
    sleep 30
    ;;
  orphan_pipe)
    # Parent exits immediately while a child keeps stdout open, so drain hangs unless
    # timeout covers wait+drain and kills the process group.
    (sleep 30) &
    printf '%s' '{"schema_version":1,"summary":"orphan","status":"done"}'
    exit 0
    ;;
  secret)
    printf '%s\n' 'TOKEN=super-secret-token-value leaked' >&2
    printf '%s' '{"schema_version":1,"summary":"echoed secret sk-abcdefghijklmnopqrstuvwxyz","status":"done"}'
    ;;
  delete)
    rm -f agent-task-delete-me.txt
    printf '%s' '{"schema_version":1,"summary":"fixture deleted file","status":"done"}'
    ;;
  *) exit 65 ;;
esac
