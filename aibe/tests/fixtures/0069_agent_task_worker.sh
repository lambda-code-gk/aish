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
    printf '%s' '{"schema_version":1,"summary":"fixture completed","reported_complete":true}'
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
  delete)
    rm -f agent-task-delete-me.txt
    printf '%s' '{"schema_version":1,"summary":"fixture deleted file","reported_complete":true}'
    ;;
  *) exit 65 ;;
esac
