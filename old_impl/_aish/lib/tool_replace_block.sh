#!/usr/bin/env bash

# functionsファイルのjson_string関数を使用するため、読み込む
. "$AISH_HOME/functions"

# 指定されたファイルのブロックを置換する関数
function replace_block
{
  local path="$1"
  local old_block="$2"
  local new_block="$3"
  
  log_tool "replace_block start: $path" "tool"

  if [ -z "$path" ]; then
    echo '{"error": "path is required"}' >&2
    return 1
  fi
  
  if [ ! -f "$path" ]; then
    jq -n --arg path "$path" '{"error": ("File not found: " + $path)}' >&2
    return 1
  fi

  # Pythonを使用して安全に置換を行う
  # 置換前にファイルの状態を保存して、diffを生成できるようにする
  local tmp_before=$(mktemp)
  cp "$path" "$tmp_before"

  # 環境変数経由でデータを渡す
  export REPLACE_PATH="$path"
  export REPLACE_OLD="$old_block"
  export REPLACE_NEW="$new_block"

  # Pythonスクリプトで置換処理
  # エラーメッセージをキャプチャする
  local py_err_file=$(mktemp)
  python3 - <<'PYTHON_EOF' 2> "$py_err_file"
import sys
import os

path = os.environ.get('REPLACE_PATH')
old_block = os.environ.get('REPLACE_OLD')
new_block = os.environ.get('REPLACE_NEW')

try:
    with open(path, 'r') as f:
        content = f.read()
    
    count = content.count(old_block)
    
    if count == 0:
        print(f"Error: old_block not found in {path}", file=sys.stderr)
        sys.exit(2)
    elif count > 1:
        print(f"Error: old_block found multiple times ({count}) in {path}. Please provide more context.", file=sys.stderr)
        sys.exit(3)
    
    new_content = content.replace(old_block, new_block)
    
    with open(path, 'w') as f:
        f.write(new_content)
    
    sys.exit(0)
except Exception as e:
    print(f"Error: {str(e)}", file=sys.stderr)
    sys.exit(4)
PYTHON_EOF

  local exit_code=$?
  local py_err=$(cat "$py_err_file")
  rm -f "$py_err_file"
  
  unset REPLACE_PATH REPLACE_OLD REPLACE_NEW

  if [ $exit_code -eq 0 ]; then
    # 差分を生成
    local diff_output=$(diff -u "$tmp_before" "$path" 2>/dev/null || true)
    rm -f "$tmp_before"
    
    jq -n --arg path "$path" --arg diff "$diff_output" '{"success": true, "path": $path, "diff": $diff}'
    return 0
  else
    rm -f "$tmp_before"
    jq -n --arg path "$path" --arg error "$py_err" --arg code "$exit_code" '{"success": false, "path": $path, "error": $error, "exit_code": $code}'
    return 1
  fi
}

# OpenAI形式のtool定義を返す（replace_block）
function _tool_replace_block_definition_openai
{
  echo '{"type": "function", "function": {"name": "replace_block", "description": "Replace a specific block of code in a file. The old_block must match exactly one location in the file.", "parameters": {"type": "object", "properties": {"path": {"type": "string", "description": "The path to the file to modify"}, "old_block": {"type": "string", "description": "The exact block of code to be replaced"}, "new_block": {"type": "string", "description": "The new block of code to replace it with"}}, "required": ["path", "old_block", "new_block"]}}}'
}

# Gemini形式のtool定義を返す（replace_block）
function _tool_replace_block_definition_gemini
{
  echo '{"name": "replace_block", "description": "Replace a specific block of code in a file. The old_block must match exactly one location in the file. provide enough context in old_block to ensure it is unique.", "parameters": {"type": "object", "properties": {"path": {"type": "string", "description": "The path to the file to modify"}, "old_block": {"type": "string", "description": "The exact block of code to be replaced"}, "new_block": {"type": "string", "description": "The new block of code to replace it with"}}, "required": ["path", "old_block", "new_block"]}}'
}

# tool execution (replace_block)
function _tool_replace_block_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  path=$(echo "$func_args" | jq -r '.path')
  old_block=$(echo "$func_args" | jq -r '.old_block')
  new_block=$(echo "$func_args" | jq -r '.new_block')
  
  if [ -z "$path" ] || [ "$path" = "null" ]; then
    echo '{"error": "path is required"}' >&2
    return 1
  fi
  
  if [ -z "$old_block" ] || [ "$old_block" = "null" ]; then
    echo '{"error": "old_block is required"}' >&2
    return 1
  fi

  if [ "$new_block" = "null" ]; then
    new_block=""
  fi
  
  # ブロックを置換
  result=$(replace_block "$path" "$old_block" "$new_block")
  
  if [ $? -ne 0 ]; then
    echo "$result" >&2
    return 1
  fi
  
  echo "$result"
}
