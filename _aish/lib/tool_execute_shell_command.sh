#!/usr/bin/env bash

# functionsファイルのjson_string関数を使用するため、読み込む
. "$AISH_HOME/functions"

# agent_approve.shの関数を使用するため、読み込む
. "$AISH_HOME/lib/agent_approve.sh"

# エラーハンドリングとログライブラリを読み込む
. "$AISH_HOME/lib/error_handler.sh"
. "$AISH_HOME/lib/logger.sh"

# 監査ログライブラリを読み込む（エラー時はスキップ）
if [ -f "$AISH_HOME/lib/audit_logger.sh" ]; then
    . "$AISH_HOME/lib/audit_logger.sh" 2>/dev/null || true
    # コンポーネント名を固定したヘルパー関数
    _audit() {
        audit_log_with_fields_safe "$1" "tool_execute_shell_command" "${@:2}"
    }
fi

# シェルコマンドを実行し、結果をJSON形式で返す
function execute_shell_command
{
  command=$1
  max_output_length=${2:-10000}
  
  # 承認済みコマンドリストのファイル
  approved_commands_file="$AISH_SESSION/approved_commands"
  
  # 確認不要コマンドかチェック
  if is_command_approved "$command"; then
    # 確認をスキップして実行
    # 監査ログ記録: 自動承認（global_list）
    _audit "command_approval" \
      "command" "$command" \
      "--metadata" "approval_method" "global_list" "approval_status" "auto_approved"
  # 承認済みコマンドかチェック
  elif [ -f "$approved_commands_file" ] && grep -Fxq "$command" "$approved_commands_file" 2>/dev/null; then
    # 確認をスキップして実行
    # 監査ログ記録: セッションリスト承認
    _audit "command_approval" \
      "command" "$command" \
      "--metadata" "approval_method" "session_list" "approval_status" "session_approved"
  else
    # ユーザーに確認を求める
    echo "" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo "🔧 Agent wants to execute command:" >&2
    echo "   $command" >&2
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
    echo -n "Execute? ([Enter] once / (A)pprove always / (N)o): " >&2
    read -r confirm < /dev/tty
    
    case "$confirm" in
      "" | [Yy] | [Yy][Ee][Ss])
        # 今回のみ実行（リストに追加しない）
        # 監査ログ記録: ユーザー承認（1回のみ）
        _audit "command_approval" \
          "command" "$command" \
          "--metadata" "approval_method" "user_interaction" "approval_status" "user_approved"
        ;;
      [Aa] | [Aa][Pp][Pp][Rr][Oo][Vv][Ee])
        # 承認済みリストに追加（永続的に許可）
        if [ ! -f "$approved_commands_file" ]; then
          touch "$approved_commands_file"
        fi
        echo "$command" >> "$approved_commands_file"
        # 監査ログ記録: ユーザー承認（永続）
        _audit "command_approval" \
          "command" "$command" \
          "--metadata" "approval_method" "user_interaction" "approval_status" "session_approved"
        ;;
      *)
        # 中止
        # 監査ログ記録: コマンド拒否
        _audit "command_rejection" \
          "command" "$command" \
          "--metadata" "approval_status" "user_rejected"
        echo '{"exit_code": 1, "stdout": "", "stderr": "Command execution was cancelled by user"}'
        return 1
        ;;
    esac
  fi
  
  # 実行するコマンドをログに記録
  log_info "Executing shell command" "tool_execute_shell_command" "$(jq -n --arg cmd "$command" '{command: $cmd}' 2>/dev/null || echo '{}')"
  log_tool "Executing: $command" "tool"
  
  # コマンドを実行（stdoutとstderrを分離）
  stdout_file=$(mktemp "$AISH_SESSION/stdout_XXXXXX")
  stderr_file=$(mktemp "$AISH_SESSION/stderr_XXXXXX")
  
  bash -c "$command" > "$stdout_file" 2> "$stderr_file"
  exit_code=$?
  
  stdout_size=$(wc -c < "$stdout_file")
  if [ "$stdout_size" -gt "$max_output_length" ]; then
    stdout=$(head -c "$max_output_length" "$stdout_file")
    stdout+=$'\n\n[... Output truncated due to size limit ('"$max_output_length"' bytes) ...]'
  else
    stdout=$(cat "$stdout_file")
  fi

  stderr_size=$(wc -c < "$stderr_file")
  if [ "$stderr_size" -gt "$max_output_length" ]; then
    stderr=$(head -c "$max_output_length" "$stderr_file")
    stderr+=$'\n\n[... Output truncated due to size limit ('"$max_output_length"' bytes) ...]'
  else
    stderr=$(cat "$stderr_file")
  fi
  
  rm -f "$stdout_file" "$stderr_file"
  
  # 監査ログ記録: コマンド実行
  _audit "command_execution" \
    "command" "$command" \
    "exit_code" "$exit_code" \
    "stdout_size" "$stdout_size" \
    "stderr_size" "$stderr_size"
  
  # JSON形式で返す
  result="{\"exit_code\": $exit_code, \"stdout\": $(echo -n "$stdout" | json_string), \"stderr\": $(echo -n "$stderr" | json_string)}"
  echo "$result"
}

# OpenAI形式のtool定義を返す
function _tool_execute_shell_command_definition_openai
{
  echo '{"type": "function", "function": {"name": "execute_shell_command", "description": "Execute a shell command and return the result with exit code, stdout, and stderr.", "parameters": {"type": "object", "properties": {"command": {"type": "string", "description": "The shell command to execute"}, "max_output_length": {"type": "integer", "description": "Maximum number of bytes to return from stdout and stderr (default: 10000). A reasonable size is 10000 to balance context usage and information.", "default": 10000}}, "required": ["command"]}}}'
}

# Gemini形式のtool定義を返す
function _tool_execute_shell_command_definition_gemini
{
  echo '{"name": "execute_shell_command", "description": "Execute a shell command and return the result with exit code, stdout, and stderr.", "parameters": {"type": "object", "properties": {"command": {"type": "string", "description": "The shell command to execute"}, "max_output_length": {"type": "integer", "description": "Maximum number of bytes to return from stdout and stderr (default: 10000). A reasonable size is 10000 to balance context usage and information.", "default": 10000}}, "required": ["command"]}}'
}

# tool実行処理
# 引数: tool_call_id - tool call ID（OpenAI形式のみ使用）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _tool_execute_shell_command_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  command=$(echo "$func_args" | jq -r '.command')
  max_output_length=$(echo "$func_args" | jq -r '.max_output_length // empty')
  
  if [ -z "$command" ]; then
    echo '{"error": "command is required"}' >&2
    return 1
  fi
  
  # シェルコマンドを実行
  result=$(execute_shell_command "$command" "$max_output_length")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}
