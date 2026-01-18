#!/usr/bin/env bash

# タスク管理ライブラリ
# タスクの検索、検証、実行、一覧表示を提供します

# タスク名のセキュリティチェック
# 引数: task_name - 検証するタスク名
# 戻り値: 0=正常、非0=エラー（エラー時はメッセージも出力）
function task_validate_name
{
  local task_name="$1"
  
  if [ -z "$task_name" ]; then
    return 0
  fi
  
  # Security check: prevent directory traversal
  if [[ "$task_name" == /* ]] || [[ "$task_name" == *..* ]]; then
    error_error "Invalid task name: $task_name" '{"component": "task"}'
    return 1
  fi
  
  return 0
}

# タスクのパスを解決する
# 引数: task_name - 解決するタスク名
# 戻り値: 解決されたタスクパス（標準出力）、見つからない場合は空文字列
function task_resolve_path
{
  local task_name="$1"
  
  if [ -z "$task_name" ]; then
    return 1
  fi
  
  # 1. Directory (legacy)
  if [ -d "$AISH_HOME/task.d/$task_name" ] && [ -f "$AISH_HOME/task.d/$task_name/execute" ]; then
    echo "$AISH_HOME/task.d/$task_name"
    return 0
  # 2. File exactly as specified
  elif [ -f "$AISH_HOME/task.d/$task_name" ]; then
    echo "$AISH_HOME/task.d/$task_name"
    return 0
  # 3. File with .sh suffix
  elif [ -f "$AISH_HOME/task.d/$task_name.sh" ]; then
    echo "$AISH_HOME/task.d/$task_name.sh"
    return 0
  fi
  
  return 1
}

# デフォルトタスクを検索する
# 戻り値: デフォルトタスクのパス（標準出力）、見つからない場合は空文字列
function task_find_default
{
  if [ -d "$AISH_HOME/task.d/default" ] && [ -f "$AISH_HOME/task.d/default/execute" ]; then
    echo "$AISH_HOME/task.d/default"
    return 0
  elif [ -f "$AISH_HOME/task.d/default.sh" ]; then
    echo "$AISH_HOME/task.d/default.sh"
    return 0
  elif [ -f "$AISH_HOME/task.d/default" ]; then
    echo "$AISH_HOME/task.d/default"
    return 0
  fi
  
  return 1
}

# すべてのタスクをリストアップする（ヘルプ表示用）
# 戻り値: タスク名と説明のリスト（標準出力）
function task_list_all
{
  (
    # Directory tasks
    find "$AISH_HOME/task.d" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | while read -r task_dir; do
      task_name=$(basename "$task_dir")
      if [ -f "$task_dir/conf" ] && [ -f "$task_dir/execute" ]; then
        desc=$( ( . "$task_dir/conf"; echo "$description" ) 2>/dev/null || echo "")
        echo -e "  $task_name\t$desc"
      fi
    done

    # Script tasks (.sh)
    find "$AISH_HOME/task.d" -type f -name "*.sh" 2>/dev/null | while read -r script_file; do
      rel_path=${script_file#$AISH_HOME/task.d/}
      task_name=${rel_path%.sh}
      desc=$(grep -m 1 "^# Description:" "$script_file" 2>/dev/null | sed 's/^# Description:[[:space:]]*//' || echo "")
      echo -e "  $task_name\t$desc"
    done

    # Plain files (without extension)
    find "$AISH_HOME/task.d" -type f ! -name "*.*" 2>/dev/null | while read -r plain_file; do
      rel_path=${plain_file#$AISH_HOME/task.d/}
      [ "$(basename "$rel_path")" == "execute" ] && continue
      [ "$(basename "$rel_path")" == "conf" ] && continue
      task_name="$rel_path"
      desc=$(grep -m 1 "^# Description:" "$plain_file" 2>/dev/null | sed 's/^# Description:[[:space:]]*//' || echo "")
      echo -e "  $task_name\t$desc"
    done
  ) | sort -u | column -t -s $'\t'
}

# タスクを実行する
# 引数: task_path - 実行するタスクのパス
function task_execute
{
  local task_path="$1"
  
  if [ -z "$task_path" ]; then
    error_error "Task path is empty" '{"component": "task"}'
    return 1
  fi
  
  if [ -d "$task_path" ]; then
    . "$task_path/conf"
    . "$task_path/execute"
  else
    # If it's a file, we just source it.
    # We used to source default/conf here, but that's legacy behavior.
    # For backward compatibility, check if it exists.
    if [ -f "$AISH_HOME/task.d/default/conf" ]; then
      . "$AISH_HOME/task.d/default/conf"
    fi
    . "$task_path"
  fi
}

