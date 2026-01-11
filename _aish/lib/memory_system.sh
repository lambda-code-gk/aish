#!/usr/bin/env bash

# metadata.json 操作ライブラリ
# 記憶システムのメタデータ管理を専門に行います

# メタデータファイルを初期化
# 引数: memory_dir - 記憶ディレクトリ
function memory_system_init
{
    local memory_dir="$1"
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
    return 0
}

# 記憶エントリをメタデータに追加
# 引数: memory_dir - 記憶ディレクトリ
#      memory_entry - 追加するエントリ (JSON文字列)
function memory_system_save_entry
{
    local memory_dir="$1"
    local memory_entry="$2"
    local metadata_file="$memory_dir/metadata.json"

    if [ ! -f "$metadata_file" ]; then
        memory_system_init "$memory_dir" || return 1
    fi

    local temp_metadata
    temp_metadata=$(mktemp)
    jq --argjson entry "$memory_entry" \
        '.memories += [$entry] | .last_updated = now | .memory_dir = $entry.memory_dir' \
        "$metadata_file" > "$temp_metadata"
    
    if [ $? -ne 0 ]; then
        rm -f "$temp_metadata"
        echo 'Error: Failed to update metadata' >&2
        return 1
    fi
    
    mv "$temp_metadata" "$metadata_file"
    return 0
}

# 記憶エントリをメタデータから削除
# 引数: memory_dir - 記憶ディレクトリ
#      memory_id - 削除する記憶のID
function memory_system_delete_entry
{
    local memory_dir="$1"
    local memory_id="$2"
    local metadata_file="$memory_dir/metadata.json"

    if [ ! -f "$metadata_file" ]; then
        return 0
    fi

    local temp_metadata
    temp_metadata=$(mktemp)
    jq --arg id "$memory_id" '.memories = (.memories | map(select(.id != $id))) | .last_updated = now' \
        "$metadata_file" > "$temp_metadata" 2>/dev/null
    
    if [ $? -ne 0 ]; then
        rm -f "$temp_metadata"
        echo "Error: Failed to update metadata" >&2
        return 1
    fi
    
    mv "$temp_metadata" "$metadata_file"
    return 0
}

# 全記憶エントリを読み込み
# 引数: memory_dir - 記憶ディレクトリ
# 戻り値: JSON配列形式で全エントリを返す
function memory_system_load_all
{
    local memory_dir="$1"
    local metadata_file="$memory_dir/metadata.json"

    if [ ! -f "$metadata_file" ]; then
        echo "[]"
        return 0
    fi

    jq -c '.memories // []' "$metadata_file" 2>/dev/null || echo "[]"
}

# ID指定で記憶エントリを取得
# 引数: memory_dir - 記憶ディレクトリ
#      memory_id - 取得する記憶のID
# 戻り値: JSON形式でエントリを返す（見つからない場合は空文字列）
function memory_system_get_by_id
{
    local memory_dir="$1"
    local memory_id="$2"
    local metadata_file="$memory_dir/metadata.json"

    if [ ! -f "$metadata_file" ]; then
        return 0
    fi

    jq -c --arg id "$memory_id" '.memories[] | select(.id == $id)' "$metadata_file" 2>/dev/null
}

