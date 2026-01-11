#!/usr/bin/env bash

# functionsファイルのjson_string関数を使用するため、読み込む
. "$AISH_HOME/functions"

# 二つのファイルを比較してunified diffを生成する関数
function diff_files
{
  local before="$1"
  local after="$2"
  
  echo "diff_files $before $after" >&2

  if [ -z "$before" ]; then
    echo '{"error": "before is required"}' >&2
    return 1
  fi
  
  if [ -z "$after" ]; then
    echo '{"error": "after is required"}' >&2
    return 1
  fi
  
  if [ ! -f "$before" ]; then
    echo '{"error": "file not found: '"$before"'"}' >&2
    return 1
  fi
  
  if [ ! -f "$after" ]; then
    echo '{"error": "file not found: '"$after"'"}' >&2
    return 1
  fi
  
  # diffコマンドでunified diff形式で比較
  diff_output=$(diff -u "$before" "$after" 2>&1)
  diff_exit_code=$?
  
  # diffの終了コード: 0=同じ, 1=異なる, 2=エラー
  if [ $diff_exit_code -eq 2 ]; then
    result="{\"error\": $(echo "$diff_output" | json_string)}"
    echo "$result" >&2
    return 1
  fi
  
  # unified diff形式で返す（ファイルが同じ場合は空文字列）
  result="{\"diff\": $(echo "$diff_output" | json_string)}"
  echo "$result"
}

# OpenAI形式のtool定義を返す
function _tool_diff_files_definition_openai
{
  echo '{"type": "function", "function": {"name": "diff_files", "description": "Compare two files and return a unified diff. Returns the differences between the files in unified diff format.", "parameters": {"type": "object", "properties": {"before": {"type": "string", "description": "The path to the first file (before)"}, "after": {"type": "string", "description": "The path to the second file (after)"}}, "required": ["before", "after"]}}}'
}

# Gemini形式のtool定義を返す
function _tool_diff_files_definition_gemini
{
  echo '{"name": "diff_files", "description": "Compare two files and return a unified diff. Returns the differences between the files in unified diff format.", "parameters": {"type": "object", "properties": {"before": {"type": "string", "description": "The path to the first file (before)"}, "after": {"type": "string", "description": "The path to the second file (after)"}}, "required": ["before", "after"]}}'
}

# tool実行処理
# 引数: tool_call_id - tool call ID（OpenAI形式のみ使用）
#      func_args - 関数引数（JSON文字列）
#      provider - "openai" または "gemini"
# 戻り値: tool実行結果（JSON形式）
function _tool_diff_files_execute
{
  local tool_call_id="$1"
  local func_args="$2"
  local provider="$3"
  
  before=$(echo "$func_args" | jq -r '.before')
  after=$(echo "$func_args" | jq -r '.after')
  
  if [ -z "$before" ] || [ "$before" = "null" ]; then
    echo '{"error": "before is required"}' >&2
    return 1
  fi
  
  if [ -z "$after" ] || [ "$after" = "null" ]; then
    echo '{"error": "after is required"}' >&2
    return 1
  fi
  
  # ファイルを比較
  result=$(diff_files "$before" "$after")
  
  if [ $? -ne 0 ]; then
    return 1
  fi
  
  echo "$result"
}

