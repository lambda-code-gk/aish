#!/usr/bin/env bash

# functionsファイルのjson_string関数を使用するため、読み込む
. "$AISH_HOME/functions"

# エラーハンドリングとログライブラリを読み込む
. "$AISH_HOME/lib/error_handler.sh"
. "$AISH_HOME/lib/logger.sh"

# ファイルを読み込む関数
function read_file
{
  local path="$1"
  local start_line="${2:-}"
  local end_line="${3:-}"

  log_info "Reading file" "tool_read_file" "$(jq -n --arg path "$path" --arg start "$start_line" --arg end "$end_line" '{path: $path, start_line: $start, end_line: $end}' 2>/dev/null || echo '{}')"
  log_tool "read_file: $path, $start_line, $end_line" "tool"

  if [ -z "$path" ]; then
    error_error "path is required" '{"component": "tool_read_file", "function": "read_file"}'
    return 1
  fi
  
  if [ ! -f "$path" ]; then
    error_error "file not found: $path" '{"component": "tool_read_file", "function": "read_file", "path": "'"$path"'"}'
    return 1
  fi
  
  # 範囲指定がある場合
  if [ ! -z "$start_line" ] && [ ! -z "$end_line" ]; then
    # start_lineとend_lineが数値か確認
    if ! [[ "$start_line" =~ ^[0-9]+$ ]] || ! [[ "$end_line" =~ ^[0-9]+$ ]]; then
      echo '{"error": "start_line and end_line must be positive integers"}' >&2
      return 1
    fi
    
    # start_lineがend_lineより大きい場合はエラー
    if [ "$start_line" -gt "$end_line" ]; then
      echo '{"error": "start_line must be less than or equal to end_line"}' >&2
      return 1
    fi
    
    # sedで指定範囲を取得（1-based index）
    content=$(sed -n "${start_line},${end_line}p" "$path")
  else
    # 範囲指定がない場合はファイル全体を読み込む
    content=$(cat "$path")
  fi
  
  # JSON形式で返す
  result="{\"content\": $(echo "$content" | json_string)}"
  echo "$result"
}

# OpenAI形式のtool定義を返す
function _tool_read_file_definition_openai
{
  echo '{"type": "function", "function": {"name": "read_file", "description": "Read the contents of a file. Returns the file content, optionally limited to a specified line range.", "parameters": {"type": "object", "properties": {"path": {"type": "string", "description": "The path to the file to read"}, "start_line": {"type": "integer", "description": "Optional: starting line number (1-based)"}, "end_line": {"type": "integer", "description": "Optional: ending line number (1-based)"}}, "required": ["path"]}}}'
}

# Gemini形式のtool定義を返す
function _tool_read_file_definition_gemini
{
  echo '{"name": "read_file", "description": "Read the contents of a file. Returns the file content, optionally limited to a specified line range.", "parameters": {"type": "object", "properties": {"path": {"type": "string", "description": "The path to the file to read"}, "start_line": {"type": "integer", "description": "Optional: starting line number (1-based)"}, "end_line": {"type": "integer", "description": "Optional: ending line number (1-based)"}}, "required": ["path"]}}'
}

# tool実行処理
# 引数: tool_call_id - tool call ID（OpenAI形式のみ使用）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _tool_read_file_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  path=$(echo "$func_args" | jq -r '.path')
  start_line=$(echo "$func_args" | jq -r '.start_line // empty')
  end_line=$(echo "$func_args" | jq -r '.end_line // empty')
  
  if [ -z "$path" ] || [ "$path" = "null" ]; then
    echo '{"error": "path is required"}' >&2
    return 1
  fi
  
  # ファイルを読み込む
  result=$(read_file "$path" "$start_line" "$end_line")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

