#!/usr/bin/env bash

# 記憶管理ライブラリ
# プロジェクト単位で記憶を分離できる階層的記憶システムを提供します

# プロジェクト固有の記憶ディレクトリを検索
# 現在の作業ディレクトリから上に辿って .aish/memory を探す
# 見つからなければ $AISH_HOME/memory を返す
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
    echo "$AISH_HOME/memory"
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
    mkdir -p "$memory_dir/by_category"
    mkdir -p "$memory_dir/by_keyword"
    
    # メタデータファイルが存在しない場合は作成
    local metadata_file="$memory_dir/metadata.json"
    if [ ! -f "$metadata_file" ]; then
        jq -n \
            --arg memory_dir "$memory_dir" \
            '{
                memories: [],
                last_updated: null,
                memory_dir: $memory_dir
            }' > "$metadata_file"
        
        if [ $? -ne 0 ]; then
            echo "Error: Failed to create metadata.json" >&2
            return 1
        fi
    fi
    
    # キーワードインデックスファイルが存在しない場合は作成
    local keyword_file="$memory_dir/by_keyword/keywords.json"
    if [ ! -f "$keyword_file" ]; then
        echo '{}' > "$keyword_file"
        if [ $? -ne 0 ]; then
            echo "Error: Failed to create keywords.json" >&2
            return 1
        fi
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
# 戻り値: JSON形式で memory_id, memory_dir, project_root を返す
function save_memory
{
    local content="$1"
    local category="${2:-general}"
    local keywords="${3:-}"
    
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
    
    local metadata_file="$memory_dir/metadata.json"
    local keyword_file="$memory_dir/by_keyword/keywords.json"
    
    # 記憶IDを生成（タイムスタンプとランダム文字列の組み合わせ）
    local memory_id
    memory_id=$(date +%s%N | sha256sum | cut -d' ' -f1 | cut -c1-16)
    local timestamp
    timestamp=$(date -Iseconds)
    local project_root
    project_root=$(dirname "$memory_dir" 2>/dev/null | xargs dirname 2>/dev/null || echo "")
    
    # キーワードを配列に変換（trim関数がない場合の代替実装）
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
        --arg timestamp "$timestamp" \
        --arg memory_dir "$memory_dir" \
        --arg project_root "$project_root" \
        '{
            id: $id,
            content: $content,
            category: $category,
            keywords: $keywords,
            timestamp: $timestamp,
            usage_count: 0,
            memory_dir: $memory_dir,
            project_root: $project_root
        }')
    
    if [ $? -ne 0 ]; then
        echo '{"error": "Failed to create memory entry"}' >&2
        return 1
    fi
    
    # カテゴリ別ファイルに保存
    local category_file="$memory_dir/by_category/${category}_${memory_id}.json"
    echo "$memory_entry" > "$category_file"
    if [ $? -ne 0 ]; then
        echo '{"error": "Failed to save category file"}' >&2
        return 1
    fi
    
    # メタデータに追加
    local temp_metadata
    temp_metadata=$(mktemp)
    jq --argjson entry "$memory_entry" \
        '.memories += [$entry] | .last_updated = now | .memory_dir = $entry.memory_dir' \
        "$metadata_file" > "$temp_metadata"
    
    if [ $? -ne 0 ]; then
        rm -f "$temp_metadata"
        echo '{"error": "Failed to update metadata"}' >&2
        return 1
    fi
    
    mv "$temp_metadata" "$metadata_file"
    
    # キーワードインデックスを更新
    if [ "$keywords_array" != "[]" ]; then
        echo "$keywords_array" | jq -r '.[]' | while IFS= read -r keyword; do
            if [ ! -z "$keyword" ]; then
                local temp_keyword
                temp_keyword=$(mktemp)
                jq --arg kw "$keyword" --arg id "$memory_id" \
                    '.[$kw] = ((.[$kw] // []) + [$id] | unique)' \
                    "$keyword_file" > "$temp_keyword"
                
                if [ $? -eq 0 ]; then
                    mv "$temp_keyword" "$keyword_file"
                else
                    rm -f "$temp_keyword"
                fi
            fi
        done
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

    # 呼び出しを表示
    echo "Searching memory: $query" >&2
    
    local project_memory_dir
    project_memory_dir=$(find_memory_directory)
    local global_memory_dir="$AISH_HOME/memory"
    
    local results="[]"
    local project_results="[]"
    local global_results="[]"
    
    # 1. プロジェクト固有の記憶を検索
    if [ -f "$project_memory_dir/metadata.json" ]; then
        # クエリを小文字に変換（bash側で実行）
        local query_lower=$(echo "$query" | tr '[:upper:]' '[:lower:]')
        
        if [ "$include_content" = "false" ]; then
            # メタデータのみを返す（contentを除外）
            project_results=$(jq -c --arg query_lower "$query_lower" --arg cat "$category" --argjson limit "$limit" '
                [.memories[] | select(.category == $cat or $cat == "") | 
                  # マッチング判定: クエリがキーワードを含んでいるか、またはコンテンツがクエリを含んでいるか
                  select(
                    (any(.keywords[]; . as $kw | $query_lower | contains($kw | ascii_downcase))) or 
                    (.content | ascii_downcase | contains($query_lower))
                  ) |
                  # スコアを事前に計算
                  . + {
                    _score: (([.keywords[] | select(. as $kw | $query_lower | contains($kw | ascii_downcase))] | length) +
                            (if (.content | ascii_downcase | contains($query_lower)) then 1 else 0 end))
                  } |
                  {
                    id: .id,
                    category: .category,
                    keywords: .keywords,
                    score: ._score,
                    source: "project",
                    memory_dir: .memory_dir,
                    project_root: .project_root
                  }] |
                  sort_by(-.score) | .[0:($limit | tonumber)]' \
                "$project_memory_dir/metadata.json" 2>/dev/null || echo "[]")
        else
            # contentを含める（既存の動作）
            project_results=$(jq -c --arg query_lower "$query_lower" --arg cat "$category" --argjson limit "$limit" '
                [.memories[] | select(.category == $cat or $cat == "") | 
                  # マッチング判定: クエリがキーワードを含んでいるか、またはコンテンツがクエリを含んでいるか
                  select(
                    (any(.keywords[]; . as $kw | $query_lower | contains($kw | ascii_downcase))) or 
                    (.content | ascii_downcase | contains($query_lower))
                  ) |
                  # スコアを事前に計算
                  . + {
                    _score: (([.keywords[] | select(. as $kw | $query_lower | contains($kw | ascii_downcase))] | length) +
                            (if (.content | ascii_downcase | contains($query_lower)) then 1 else 0 end))
                  } |
                  {
                    id: .id,
                    category: .category,
                    content: .content,
                    keywords: .keywords,
                    score: ._score,
                    source: "project",
                    memory_dir: .memory_dir,
                    project_root: .project_root
                  }] |
                  sort_by(-.score) | .[0:($limit | tonumber)]' \
                "$project_memory_dir/metadata.json" 2>/dev/null || echo "[]")
        fi
        
        if [ "$project_results" != "null" ] && [ "$project_results" != "" ]; then
            results="$project_results"
        fi
    fi
    
    # 2. グローバルの記憶も検索（プロジェクト固有とは別のディレクトリの場合）
    if [ "$project_memory_dir" != "$global_memory_dir" ] && [ -f "$global_memory_dir/metadata.json" ]; then
        # クエリを小文字に変換（bash側で実行）
        local query_lower=$(echo "$query" | tr '[:upper:]' '[:lower:]')
        
        if [ "$include_content" = "false" ]; then
            # メタデータのみを返す（contentを除外）
            global_results=$(jq -c --arg query_lower "$query_lower" --arg cat "$category" --argjson limit "$limit" '
                [.memories[] | select(.category == $cat or $cat == "") | 
                  select(
                    (any(.keywords[]; . as $kw | $query_lower | contains($kw | ascii_downcase))) or 
                    (.content | ascii_downcase | contains($query_lower))
                  ) |
                  . + {
                    _score: (([.keywords[] | select(. as $kw | $query_lower | contains($kw | ascii_downcase))] | length) +
                            (if (.content | ascii_downcase | contains($query_lower)) then 1 else 0 end))
                  } |
                  {
                    id: .id,
                    category: .category,
                    keywords: .keywords,
                    score: ._score,
                    source: "global",
                    memory_dir: .memory_dir,
                    project_root: .project_root
                  }] |
                  sort_by(-.score) | .[0:($limit | tonumber)]' \
                "$global_memory_dir/metadata.json" 2>/dev/null || echo "[]")
        else
            # contentを含める（既存の動作）
            global_results=$(jq -c --arg query_lower "$query_lower" --arg cat "$category" --argjson limit "$limit" '
                [.memories[] | select(.category == $cat or $cat == "") | 
                  select(
                    (any(.keywords[]; . as $kw | $query_lower | contains($kw | ascii_downcase))) or 
                    (.content | ascii_downcase | contains($query_lower))
                  ) |
                  . + {
                    _score: (([.keywords[] | select(. as $kw | $query_lower | contains($kw | ascii_downcase))] | length) +
                            (if (.content | ascii_downcase | contains($query_lower)) then 1 else 0 end))
                  } |
                  {
                    id: .id,
                    category: .category,
                    content: .content,
                    keywords: .keywords,
                    score: ._score,
                    source: "global",
                    memory_dir: .memory_dir,
                    project_root: .project_root
                  }] |
                  sort_by(-.score) | .[0:($limit | tonumber)]' \
                "$global_memory_dir/metadata.json" 2>/dev/null || echo "[]")
        fi
        
        if [ "$global_results" != "null" ] && [ "$global_results" != "" ]; then
            # 両方の結果をマージ（プロジェクト固有を優先、重複を除外）
            if [ "$results" = "[]" ]; then
                results="$global_results"
            else
                # プロジェクト結果とグローバル結果をマージ
                # 同じIDまたは同じ内容の場合はプロジェクト固有を優先
                if [ "$include_content" = "false" ]; then
                    # contentがない場合はIDのみで重複除去
                    results=$(echo "$results $global_results" | jq -s --argjson limit "$limit" '
                        flatten |
                        # IDで重複を除去（プロジェクト固有を優先）
                        group_by(.id) | map(.[0]) |
                        # scoreでソート
                        sort_by(-.score) |
                        # limitを適用
                        .[0:($limit | tonumber)]
                    ')
                else
                    # contentがある場合は従来通り
                    results=$(echo "$results $global_results" | jq -s --argjson limit "$limit" '
                        flatten |
                        # IDで重複を除去（プロジェクト固有を優先）
                        group_by(.id) | map(.[0]) |
                        # contentで重複を除去（プロジェクト固有を優先）
                        group_by(.content) | map(.[0]) |
                        # scoreでソート
                        sort_by(-.score) |
                        # limitを適用
                        .[0:($limit | tonumber)]
                    ')
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

