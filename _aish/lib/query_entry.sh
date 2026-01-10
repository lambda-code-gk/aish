#!/usr/bin/env bash

# query関数の共通エントリーポイント
# オプション解析、ファイル準備を行う
# 結果は以下のグローバル変数に設定される:
#   _query_system_instruction: システムインストラクション
#   _query_agent_mode: エージェントモード（true/false）
#   _query_files: 準備されたファイルリスト
#   _query_args: 残りの引数
function query_entry_prepare
{
  _query_system_instruction=""
  _query_agent_mode=false
  
  # OPTINDをリセット（再呼び出しに対応）
  OPTIND=1
  
  while getopts "s:a" opt; do
    case $opt in
      s) _query_system_instruction=$OPTARG ;;
      a) _query_agent_mode=true ;;
      *)  ;;
    esac
  done
  shift $((OPTIND - 1))
  
  # 残りの引数を保存
  _query_args="$*"
  
  # 関連する記憶を検索して注入
  if [ ! -z "$_query_args" ]; then
    # 記憶管理ライブラリを読み込む
    [ -f "$AISH_HOME/lib/memory_manager.sh" ] && . "$AISH_HOME/lib/memory_manager.sh"
    
    # 検索を実行
    local memories=$(search_memory_efficient "$_query_args" "" 3)
    
    if [ ! -z "$memories" ] && [ "$memories" != "[]" ]; then
        local memory_text=$(echo "$memories" | jq -r '
            "### Relevant Knowledge from Past Interactions:\n" +
            ([.[] | "- [" + .category + "] " + .content + " (Keywords: " + (.keywords | join(", ")) + ")"] | join("\n"))
        ')
        
        if [ ! -z "$memory_text" ]; then
            # システムインストラクションの前に追加
            if [ -z "$_query_system_instruction" ]; then
                _query_system_instruction="$memory_text"
            else
                _query_system_instruction="$memory_text\n\nOriginal Instructions:\n$_query_system_instruction"
            fi
        fi
    fi
  fi
  
  aish_rollout
  
  _query_files=$(detail.aish_list_parts | detail.aish_security_check)
  if [ $? -ne 0 ]; then
    exit 1
  fi
}

# レスポンステキストをファイルに保存して標準出力にも出力
# 引数: text - 保存・出力するテキスト
function save_response_text
{
  local text="$1"
  echo "$text" | tee "$AISH_PART/part_$(date +%Y%m%d_%H%M%S)_assistant.txt"
}

