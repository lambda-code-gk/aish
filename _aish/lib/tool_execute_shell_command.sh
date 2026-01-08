#!/usr/bin/env bash

# agent_approve.shの関数を使用するため、読み込む
. "$AISH_HOME/lib/agent_approve.sh"

# シェルコマンドを実行し、結果をJSON形式で返す
function execute_shell_command
{
  command=$1
  
  # 承認済みコマンドリストのファイル
  approved_commands_file="$AISH_SESSION/approved_commands"
  
  # 確認不要コマンドかチェック
  if is_command_approved "$command"; then
    # 確認をスキップして実行
    :
  # 承認済みコマンドかチェック
  elif [ -f "$approved_commands_file" ] && grep -Fxq "$command" "$approved_commands_file" 2>/dev/null; then
    # 確認をスキップして実行
    :
  else
    # ユーザーに確認を求める
    echo "" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "🔧 Agent wants to execute command:" >&2
    echo "   $command" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo -n "Execute? ([y]es / [n]o): " >&2
    read -r confirm < /dev/tty
    
    if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
      echo '{"exit_code": 1, "stdout": "", "stderr": "Command execution was cancelled by user"}'
      return 1
    fi
    
    # 承認済みリストに追加（ファイルが存在しない場合は作成）
    if [ ! -f "$approved_commands_file" ]; then
      touch "$approved_commands_file"
    fi
    echo "$command" >> "$approved_commands_file"
  fi
  
  # 実行するコマンドを標準エラー出力に表示
  echo "Executing: $command" >&2
  
  # コマンドを実行（stdoutとstderrを分離）
  stdout_file=$(mktemp "$AISH_SESSION/stdout_XXXXXX")
  stderr_file=$(mktemp "$AISH_SESSION/stderr_XXXXXX")
  
  bash -c "$command" > "$stdout_file" 2> "$stderr_file"
  exit_code=$?
  
  stdout=$(cat "$stdout_file")
  stderr=$(cat "$stderr_file")
  
  rm -f "$stdout_file" "$stderr_file"
  
  # JSON形式で返す
  result="{\"exit_code\": $exit_code, \"stdout\": $(echo "$stdout" | json_string), \"stderr\": $(echo "$stderr" | json_string)}"
  echo "$result"
}

