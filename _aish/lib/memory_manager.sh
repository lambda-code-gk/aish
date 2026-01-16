#!/usr/bin/env bash

# 記憶管理ライブラリ
# プロジェクト単位で記憶を分離できる階層的記憶システムを提供します

source "$(dirname "${BASH_SOURCE[0]}")/memory_system.sh"

# プロジェクト固有の記憶ディレクトリを検索
# 現在の作業ディレクトリから上に辿って .aish/memory を探す
# 見つからなければ ${AISH_HOME:-}/memory を返す
function find_memory_directory
{
    local current_dir
    current_dir=$(pwd)
    
    # 現在のディレクトリから上に辿って .aish/memory を探す
    while [ "$current_dir" != "/" ]; do
        if [ -d "$current_dir/.aish/memory" ]; then
            echo "$current_dir/.aish/memory"
            return 0
        fi
        current_dir=$(dirname "$current_dir")
    done
    
    # 見つからなければグローバルの記憶ディレクトリを使用
    echo "${AISH_HOME:-}/memory"
}

# 記憶ディレクトリを初期化
# 引数: memory_dir - 初期化する記憶ディレクトリのパス
function init_memory_directory
{
    local memory_dir="$1"
    
    if [ -z "$memory_dir" ]; then
        echo "Error: memory_dir is required" >&2
        return 1
    fi
    
    # ディレクトリを作成
    mkdir -p "$memory_dir/entries"
    
    # メタデータファイルが存在しない場合は作成
    memory_system_init "$memory_dir"
    if [ $? -ne 0 ]; then
        return 1
    fi
}

# 現在使用すべき記憶ディレクトリを取得して初期化
# 戻り値: 記憶ディレクトリのパス（標準出力）
function get_memory_directory
{
    local memory_dir
    memory_dir=$(find_memory_directory)
    
    if [ $? -ne 0 ]; then
        echo "Error: Failed to find memory directory" >&2
        return 1
    fi
    
    init_memory_directory "$memory_dir"
    if [ $? -ne 0 ]; then
        echo "Error: Failed to initialize memory directory" >&2
        return 1
    fi
    
    echo "$memory_dir"
}

# 記憶を保存
# 引数: content - 記憶する内容
#      category - カテゴリ（デフォルト: "general"）
#      keywords - キーワード（カンマ区切り、デフォルト: 空）
#      subject - 主題・タイトル（デフォルト: 空）
# 戻り値: JSON形式で memory_id, memory_dir, project_root を返す
function save_memory
{
    local content="$1"
    local category="${2:-general}"
    local keywords="${3:-}"
    local subject="${4:-}"
    
    if [ -z "$content" ]; then
        echo '{"error": "content is required"}' >&2
        return 1
    fi

    # 呼び出しを表示（1行で表示、長い場合は省略）
    local content_summary=$(echo "$content" | head -n 1 | cut -c1-100)
    [ ${#content} -gt 100 ] && content_summary="${content_summary}..."
    echo "Saving memory: [$category] $content_summary" >&2
    
    # 現在のディレクトリに応じた記憶ディレクトリを取得
    local memory_dir
    memory_dir=$(get_memory_directory)
    if [ $? -ne 0 ]; then
        echo '{"error": "Failed to get memory directory"}' >&2
        return 1
    fi
    
    # 記憶IDを生成（タイムスタンプとランダム文字列の組み合わせ）
    local memory_id
    memory_id=$(date +%s%N | sha256sum | cut -d' ' -f1 | cut -c1-16)
    local timestamp
    timestamp=$(date -Iseconds)
    local project_root
    project_root=$(dirname "$memory_dir" 2>/dev/null | xargs dirname 2>/dev/null || echo "")
    
    # キーワードを配列に変換
    local keywords_array
    if [ -z "$keywords" ]; then
        keywords_array="[]"
    else
        # カンマ区切りを配列に変換し、前後の空白を除去
        keywords_array=$(echo "$keywords" | jq -R 'split(",") | map(gsub("^\\s+|\\s+$"; "")) | map(select(. != ""))')
        if [ $? -ne 0 ]; then
            echo '{"error": "Failed to parse keywords"}' >&2
            return 1
        fi
    fi
    
    # 記憶エントリを作成
    local memory_entry
    memory_entry=$(jq -n \
        --arg id "$memory_id" \
        --arg content "$content" \
        --arg category "$category" \
        --argjson keywords "$keywords_array" \
        --arg subject "$subject" \
        --arg timestamp "$timestamp" \
        --arg memory_dir "$memory_dir" \
        --arg project_root "$project_root" \
        '{
            id: $id,
            content: $content,
            category: $category,
            keywords: $keywords,
            subject: $subject,
            timestamp: $timestamp,
            usage_count: 0,
            memory_dir: $memory_dir,
            project_root: $project_root
        }')
    
    if [ $? -ne 0 ]; then
        echo '{"error": "Failed to create memory entry"}' >&2
        return 1
    fi
    
    # エントリファイルに保存 (詳細内容含む)
    local entry_file="$memory_dir/entries/${memory_id}.json"
    echo "$memory_entry" > "$entry_file"
    if [ $? -ne 0 ]; then
        echo '{"error": "Failed to save entry file"}' >&2
        return 1
    fi
    
    # メタデータに追加 (memory_system_save_entry 内で content は除外される)
    memory_system_save_entry "$memory_dir" "$memory_entry"
    if [ $? -ne 0 ]; then
        return 1
    fi
    
    # 結果を返す
    echo "{\"memory_id\": \"$memory_id\", \"memory_dir\": \"$memory_dir\", \"project_root\": \"$project_root\"}"
}

# 記憶を検索（プロジェクト固有とグローバルの両方を検索、プロジェクト優先）
# 引数: query - 検索クエリ
#      category - カテゴリフィルタ（オプション、空の場合は全カテゴリ）
#      limit - 結果数の上限（デフォルト: 5）
#      include_content - contentを含めるかどうか（デフォルト: true、falseの場合はメタデータのみ）
# 戻り値: JSON配列形式で検索結果を返す
function search_memory_efficient
{
    local query="$1"
    local category="${2:-}"
    local limit="${3:-5}"
    local include_content="${4:-true}"
    
    if [ -z "$query" ]; then
        echo "[]"
        return 0
    fi

    # デバッグ時のみ表示
    if [ "${AISH_DEBUG:-}" = "true" ]; then
        echo "Searching memory: $query" >&2
    fi
    
    local project_memory_dir
    project_memory_dir=$(find_memory_directory)
    local global_memory_dir="${AISH_HOME:-}/memory"
    
    local results="[]"
    local project_results="[]"
    local global_results="[]"
    
    # 1. プロジェクト固有の記憶を検索
    local project_memories=$(memory_system_load_all "$project_memory_dir")
    if [ "$project_memories" != "[]" ]; then
        # クエリを小文字に変換
        local query_lower=$(echo "$query" | tr '[:upper:]' '[:lower:]')
        
        project_results=$(echo "$project_memories" | jq -c --arg query_lower "$query_lower" --arg cat "$category" --argjson limit "$limit" '
            [.[] | select(.category == $cat or $cat == "") | 
              # マッチング判定: クエリがキーワードを含んでいるか（単語境界を考慮）、または主題がクエリを含んでいるか
              # 特殊文字をエスケープして正規表現を作成
              . + {
                _matched_kws: [.keywords[] | select(. as $kw | 
                  ($kw | split("") | map(if . | test("[.\\+*?\\[\\](){}^$|]") then "\\" + . else . end) | join("")) as $escaped_kw |
                  if ($kw | length) >= 3 then
                    ($query_lower | test("\\b" + ($escaped_kw | ascii_downcase) + "\\b"))
                  else
                    ($query_lower | contains($kw | ascii_downcase))
                  end
                )]
              } |
              select((._matched_kws | length > 0) or (.subject | ascii_downcase | contains($query_lower))) |
              # スコアを計算（主題一致を優先）
              . + {
                _score: ((._matched_kws | length) +
                        (if (.subject | ascii_downcase | contains($query_lower)) then 2 else 0 end))
              } |
              {
                id: .id,
                category: .category,
                keywords: .keywords,
                subject: .subject,
                score: ._score,
                source: "project",
                memory_dir: .memory_dir,
                project_root: .project_root
              }] |
              sort_by(-.score) | .[0:($limit | tonumber)]' 2>/dev/null || echo "[]")
        
        if [ "$project_results" != "null" ] && [ "$project_results" != "" ]; then
            results="$project_results"
        fi
    fi
    
    # 2. グローバルの記憶も検索
    if [ "$project_memory_dir" != "$global_memory_dir" ]; then
        local global_memories=$(memory_system_load_all "$global_memory_dir")
        if [ "$global_memories" != "[]" ]; then
            local query_lower=$(echo "$query" | tr '[:upper:]' '[:lower:]')
            
            global_results=$(echo "$global_memories" | jq -c --arg query_lower "$query_lower" --arg cat "$category" --argjson limit "$limit" '
                [.[] | select(.category == $cat or $cat == "") | 
                  . + {
                    _matched_kws: [.keywords[] | select(. as $kw | 
                      ($kw | split("") | map(if . | test("[.\\+*?\\[\\](){}^$|]") then "\\" + . else . end) | join("")) as $escaped_kw |
                      if ($kw | length) >= 3 then
                        ($query_lower | test("\\b" + ($escaped_kw | ascii_downcase) + "\\b"))
                      else
                        ($query_lower | contains($kw | ascii_downcase))
                      end
                    )]
                  } |
                  select((._matched_kws | length > 0) or (.subject | ascii_downcase | contains($query_lower))) |
                  . + {
                    _score: ((._matched_kws | length) +
                            (if (.subject | ascii_downcase | contains($query_lower)) then 2 else 0 end))
                  } |
                  {
                    id: .id,
                    category: .category,
                    keywords: .keywords,
                    subject: .subject,
                    score: ._score,
                    source: "global",
                    memory_dir: .memory_dir,
                    project_root: .project_root
                  }] |
                  sort_by(-.score) | .[0:($limit | tonumber)]' 2>/dev/null || echo "[]")
            
            if [ "$global_results" != "null" ] && [ "$global_results" != "" ]; then
                # 両方の結果をマージ
                if [ "$results" = "[]" ]; then
                    results="$global_results"
                else
                    results=$(echo "$results $global_results" | jq -s --argjson limit "$limit" '
                        flatten |
                        group_by(.id) | map(.[0]) |
                        sort_by(-.score) |
                        .[0:($limit | tonumber)]
                    ')
                fi
            fi
        fi
    fi
    
    # include_content=true の場合は content を補充
    if [ "$include_content" = "true" ] && [ "$results" != "[]" ]; then
        # 各IDについて get_memory_content を呼び出して content を取得する
        # (効率は落ちるが、metadata.json から content を消したため必要)
        local results_with_content="[]"
        for row in $(echo "$results" | jq -r '.[] | @base64'); do
            local item=$(echo "${row}" | base64 --decode)
            local id=$(echo "${item}" | jq -r '.id')
            local full_entry=$(get_memory_content "$id" 2>/dev/null)
            if [ $? -eq 0 ]; then
                local content=$(echo "$full_entry" | jq -r '.content')
                item=$(echo "$item" | jq --arg content "$content" '. + {content: $content}')
            fi
            results_with_content=$(echo "$results_with_content" | jq --argjson item "$item" '. += [$item]')
        done
        results="$results_with_content"
    fi

    # 結果が空の場合は空配列を返す
    if [ "$results" = "null" ] || [ -z "$results" ]; then
        results="[]"
    fi
    
    echo "$results"
}

# 記憶を削除
# 引数: memory_id - 削除する記憶のID
# 戻り値: 成功時は0、失敗時は1
function revoke_memory
{
    local memory_id="$1"
    
    if [ -z "$memory_id" ]; then
        echo "Error: memory_id is required" >&2
        return 1
    fi
    
    # プロジェクト固有とグローバルの両方の記憶ディレクトリを確認
    local project_memory_dir
    project_memory_dir=$(find_memory_directory)
    local global_memory_dir="${AISH_HOME:-}/memory"
    
    local found=false
    local memory_dir_to_use=""
    local memory_entry=""
    
    # 1. プロジェクト固有の記憶を検索
    memory_entry=$(memory_system_get_by_id "$project_memory_dir" "$memory_id")
    if [ ! -z "$memory_entry" ]; then
        found=true
        memory_dir_to_use="$project_memory_dir"
    fi
    
    # 2. グローバルの記憶を検索（プロジェクト固有で見つからなかった場合）
    if [ "$found" = false ] && [ "$project_memory_dir" != "$global_memory_dir" ]; then
        memory_entry=$(memory_system_get_by_id "$global_memory_dir" "$memory_id")
        if [ ! -z "$memory_entry" ]; then
            found=true
            memory_dir_to_use="$global_memory_dir"
        fi
    fi
    
    if [ "$found" = false ]; then
        echo "Error: Memory with id '$memory_id' not found" >&2
        return 1
    fi
    
    # エントリファイルを削除
    local entry_file="$memory_dir_to_use/entries/${memory_id}.json"
    if [ -f "$entry_file" ]; then
        rm -f "$entry_file"
        if [ $? -ne 0 ]; then
            echo "Error: Failed to delete entry file: $entry_file" >&2
            return 1
        fi
    fi
    
    # メタデータから削除
    memory_system_delete_entry "$memory_dir_to_use" "$memory_id"
    if [ $? -ne 0 ]; then
        return 1
    fi
    
    echo "Memory '$memory_id' has been revoked successfully"
    return 0
}

# 記憶の一覧を取得
# 引数: memory_dir - 記憶ディレクトリ（オプション、指定しない場合は自動検出）
# 戻り値: JSON配列形式で記憶の一覧を返す（contentは含まない）
function list_memories
{
    local memory_dir="${1:-}"
    
    if [ -z "$memory_dir" ]; then
        memory_dir=$(find_memory_directory)
    fi
    
    local project_memory_dir="$memory_dir"
    local global_memory_dir="${AISH_HOME:-}/memory"
    
    local results="[]"
    
    # 1. プロジェクト固有の記憶を取得
    local project_memories=$(memory_system_load_all "$project_memory_dir")
    if [ "$project_memories" != "[]" ]; then
        local project_results
        project_results=$(echo "$project_memories" | jq -c '[.[] | {
            id: .id,
            category: .category,
            keywords: .keywords,
            subject: .subject,
            timestamp: .timestamp,
            usage_count: .usage_count,
            memory_dir: .memory_dir,
            project_root: .project_root,
            source: "project"
        }]' 2>/dev/null || echo "[]")
        
        if [ "$project_results" != "null" ] && [ "$project_results" != "" ]; then
            results="$project_results"
        fi
    fi
    
    # 2. グローバルの記憶も取得
    if [ "$project_memory_dir" != "$global_memory_dir" ]; then
        local global_memories=$(memory_system_load_all "$global_memory_dir")
        if [ "$global_memories" != "[]" ]; then
            local global_results
            global_results=$(echo "$global_memories" | jq -c '[.[] | {
                id: .id,
                category: .category,
                keywords: .keywords,
                subject: .subject,
                timestamp: .timestamp,
                usage_count: .usage_count,
                memory_dir: .memory_dir,
                project_root: .project_root,
                source: "global"
            }]' 2>/dev/null || echo "[]")
            
            if [ "$global_results" != "null" ] && [ "$global_results" != "" ]; then
                # 両方の結果をマージ
                if [ "$results" = "[]" ]; then
                    results="$global_results"
                else
                    results=$(echo "$results $global_results" | jq -s 'flatten')
                fi
            fi
        fi
    fi
    
    # 結果が空の場合は空配列を返す
    if [ "$results" = "null" ] || [ -z "$results" ]; then
        results="[]"
    fi
    
    echo "$results"
}

# 記憶の詳細情報を取得
# 引数: memory_id - 記憶のID
# 戻り値: JSON形式で記憶の詳細情報を返す
function get_memory_content
{
    local memory_id="$1"
    
    if [ -z "$memory_id" ]; then
        echo '{"error": "memory_id is required"}' >&2
        return 1
    fi
    
    # プロジェクト固有とグローバルの両方の記憶ディレクトリを確認
    local project_memory_dir
    project_memory_dir=$(find_memory_directory)
    local global_memory_dir="${AISH_HOME:-}/memory"
    
    local memory_entry=""
    
    # 1. プロジェクト固有の記憶を検索
    memory_entry=$(memory_system_get_by_id "$project_memory_dir" "$memory_id")
    if [ ! -z "$memory_entry" ]; then
        # contentを含む完全な情報をエントリファイルから取得
        local entry_file="$project_memory_dir/entries/${memory_id}.json"
        if [ -f "$entry_file" ]; then
            memory_entry=$(cat "$entry_file" 2>/dev/null)
        fi
        # sourceを追加
        memory_entry=$(echo "$memory_entry" | jq -c '. + {source: "project"}' 2>/dev/null)
        echo "$memory_entry"
        return 0
    fi
    
    # 2. グローバルの記憶を検索
    if [ "$project_memory_dir" != "$global_memory_dir" ]; then
        memory_entry=$(memory_system_get_by_id "$global_memory_dir" "$memory_id")
        if [ ! -z "$memory_entry" ]; then
            # contentを含む完全な情報をエントリファイルから取得
            local entry_file="$global_memory_dir/entries/${memory_id}.json"
            if [ -f "$entry_file" ]; then
                memory_entry=$(cat "$entry_file" 2>/dev/null)
            fi
            # sourceを追加
            memory_entry=$(echo "$memory_entry" | jq -c '. + {source: "global"}' 2>/dev/null)
            echo "$memory_entry"
            return 0
        fi
    fi
    
    echo '{"error": "Memory not found"}' >&2
    return 1
}