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
  
  # 記憶管理ライブラリを読み込む
  [ -f "$AISH_HOME/lib/memory_manager.sh" ] && . "$AISH_HOME/lib/memory_manager.sh"
  
  # metadata.jsonから全てのキーワードを抽出してコンテキストに追加
  # プロジェクト固有とグローバルの両方を確認
  local project_memory_dir
  project_memory_dir=$(find_memory_directory 2>/dev/null)
  local global_memory_dir="$AISH_HOME/memory"
  
  # 両方のmetadata.jsonからキーワードを集約
  local keywords_array="[]"
  
  # プロジェクト固有のmetadata.jsonからキーワードを取得
  if [ ! -z "$project_memory_dir" ] && [ -f "$project_memory_dir/metadata.json" ]; then
    local project_keywords=$(jq -c '[.memories[]?.keywords[]?] | unique' "$project_memory_dir/metadata.json" 2>/dev/null || echo "[]")
    if [ ! -z "$project_keywords" ] && [ "$project_keywords" != "null" ] && [ "$project_keywords" != "[]" ]; then
      keywords_array=$(echo "$keywords_array $project_keywords" | jq -s 'flatten | unique')
    fi
  fi
  
  # グローバルのmetadata.jsonからキーワードを取得（プロジェクト固有と異なる場合のみ）
  if [ "$project_memory_dir" != "$global_memory_dir" ] && [ -f "$global_memory_dir/metadata.json" ]; then
    local global_keywords=$(jq -c '[.memories[]?.keywords[]?] | unique' "$global_memory_dir/metadata.json" 2>/dev/null || echo "[]")
    if [ ! -z "$global_keywords" ] && [ "$global_keywords" != "null" ] && [ "$global_keywords" != "[]" ]; then
      keywords_array=$(echo "$keywords_array $global_keywords" | jq -s 'flatten | unique')
    fi
  fi
  
  # キーワードが存在する場合はコンテキストに追加
  local keywords_text=$(echo "$keywords_array" | jq -r 'join(", ")' 2>/dev/null)
  if [ ! -z "$keywords_text" ] && [ "$keywords_text" != "null" ] && [ "$keywords_text" != "" ]; then
    local keywords_context="### Available Knowledge Keywords:\nThese keywords are available in the memory system. Use the search_memory tool if you need detailed information about these topics:\n$keywords_text\n"
    
    # システムインストラクションの前に追加
    if [ -z "$_query_system_instruction" ]; then
      _query_system_instruction="$keywords_context"
    else
      _query_system_instruction="$keywords_context\n$_query_system_instruction"
    fi
  fi
  
  # 関連する記憶を検索して注入
  if [ ! -z "$_query_args" ]; then
    # 検索を実行
    local memories=$(search_memory_efficient "$_query_args" "" 3)
    
    if [ ! -z "$memories" ] && [ "$memories" != "[]" ]; then
        # contentが存在する場合はcontentを表示、ない場合はメタデータのみ表示
        local memory_text=$(echo "$memories" | jq -r '
            "### Relevant Knowledge from Past Interactions:\n" +
            ([.[] | 
              if has("content") and .content != null then
                "- [" + .category + "] " + .content + " (Keywords: " + (.keywords | join(", ")) + ")"
              else
                "- ID: " + .id + " [" + .category + "] Keywords: " + (.keywords | join(", ")) + " (use get_memory_content to retrieve details)"
              end
            ] | join("\n"))
        ')
        
        if [ ! -z "$memory_text" ]; then
            # システムインストラクションに追加
            _query_system_instruction="$_query_system_instruction\n\n$memory_text"
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

