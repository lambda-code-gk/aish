#!/bin/sh
set -eu

mode="${1:-success}"

# Modes that must emit output before reading stdin (pipe-deadlock regression).
case "$mode" in
  startup_spam)
    # Fill the stderr pipe before consuming stdin. Without concurrent drain,
    # the parent blocks on write_all(stdin) while this Worker blocks on write.
    i=0
    while [ "$i" -lt 200000 ]; do
      printf 'S'
      i=$((i + 1))
    done >&2
    input=$(dd bs=65536 count=1 2>/dev/null)
    for required in '"schema_version":1' '"delegation_depth":1' '"objective"' '"completion_criteria"' '"cwd"'; do
      case "$input" in
        *"$required"*) ;;
        *) printf '%s\n' 'invalid envelope' >&2; exit 64 ;;
      esac
    done
    printf '%s' '{"schema_version":1,"summary":"startup spam drained","status":"done"}'
    exit 0
    ;;
esac

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
  env_value_secret)
    # Emit the inherited credential value alone (no KEY=/TOKEN= prefix). Pattern
    # sanitize must not be the only defense — exact-value replacement is required.
    printf '%s\n' "${AWS_SECRET_ACCESS_KEY-}" >&2
    printf '%s' "{\"schema_version\":1,\"summary\":\"bare ${AWS_SECRET_ACCESS_KEY-} in summary\",\"status\":\"done\"}"
    ;;
  truncated_env_secret)
    # Pad stderr so max_output_bytes truncates in the middle of the inherited secret.
    {
      i=0
      while [ "$i" -lt 4080 ]; do
        printf x
        i=$((i + 1))
      done
      printf '%s' "${AWS_SECRET_ACCESS_KEY-}"
    } >&2
    printf '%s' '{"schema_version":1,"summary":"truncated env secret","status":"done"}'
    ;;
  secret_named_path)
    # Embed the inherited credential in a created filename so changed_paths
    # would otherwise leak it into Result/Evidence.
    touch -- "${AWS_SECRET_ACCESS_KEY-}"
    printf '%s' '{"schema_version":1,"summary":"created secret-named file","status":"done"}'
    ;;
  delete)
    rm -f agent-task-delete-me.txt
    printf '%s' '{"schema_version":1,"summary":"fixture deleted file","status":"done"}'
    ;;
  gap_repair)
    case "$input" in
      *'Gap c1:'*) printf '%s\n' 'verified' > 0070-artifact.txt ;;
      *) printf '%s\n' 'incomplete' > 0070-artifact.txt ;;
    esac
    printf '%s' '{"schema_version":1,"summary":"worker reports complete","status":"done"}'
    ;;
  *) exit 65 ;;
esac
