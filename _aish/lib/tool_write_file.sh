#!/usr/bin/env bash

# functionsファイルのjson_string関数を使用するため、読み込む
. "$AISH_HOME/functions"

# ファイルを書き込む関数
function write_file
{
  local path="$1"
  local content="$2"
  
  detail.aish_log_tool "write_file start: $path"

  if [ -z "$path" ]; then
    echo '{"error": "path is required"}' >&2
    return 1
  fi
  
  # ディレクトリが存在しない場合は作成
  mkdir -p "$(dirname "$path")"
  
  # ファイルに書き込み
  echo -n "$content" > "$path"
  
  if [ $? -eq 0 ]; then
    jq -n --arg path "$path" '{"success": true, "path": $path}'
    return 0
  else
    jq -n --arg path "$path" --arg error "Failed to write to $path" '{"success": false, "path": $path, "error": $error}'
    return 1
  fi
}

# OpenAI形式のtool定義を返す（write_file）
function _tool_write_file_definition_openai
{
  echo '{"type": "function", "function": {"name": "write_file", "description": "Write content to a file. If the file exists, it will be overwritten. If not, it will be created.", "parameters": {"type": "object", "properties": {"path": {"type": "string", "description": "The path to the file to write"}, "content": {"type": "string", "description": "The content to write to the file"}}, "required": ["path", "content"]}}}'
}

# Gemini形式のtool定義を返す（write_file）
function _tool_write_file_definition_gemini
{
  echo '{"name": "write_file", "description": "Write content to a file. If the file exists, it will be overwritten. If not, it will be created.", "parameters": {"type": "object", "properties": {"path": {"type": "string", "description": "The path to the file to write"}, "content": {"type": "string", "description": "The content to write to the file"}}, "required": ["path", "content"]}}'
}

# tool実行処理（write_file）
function _tool_write_file_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  path=$(echo "$func_args" | jq -r '.path')
  content=$(echo "$func_args" | jq -r '.content')
  
  if [ -z "$path" ] || [ "$path" = "null" ]; then
    echo '{"error": "path is required"}' >&2
    return 1
  fi
  
  if [ "$content" = "null" ]; then
    content=""
  fi
  
  # ファイルを書き込み
  result=$(write_file "$path" "$content")
  
  if [ $? -ne 0 ]; then
    echo "$result" >&2
    return 1
  fi
  
  echo "$result"
}
