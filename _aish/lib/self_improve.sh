#!/bin/bash
# 自己改善（履歴からの知見抽出・記録）用ライブラリ

# 共通ライブラリを読み込む
[ -f "$AISH_HOME/lib/memory_manager.sh" ] && . "$AISH_HOME/lib/memory_manager.sh"

# セッション履歴から要約に必要な情報を抽出する
# 引数: log_file - ログファイル(JSONL)のパス
# 戻り値: 抽出されたテキスト
function extract_session_history {
    local log_file="$1"
    
    if [ ! -f "$log_file" ]; then
        return 1
    fi

    # jqを使用して、role, content, および tool_calls の内容を読みやすい形式で抽出
    # 冗長な出力を避けるため、stdout/stderrが長い場合は切り詰めるなどの処理が必要だが、
    # ここではまず基本的な情報を抽出する
    jq -r '
        if .role == "user" then
            "USER: " + .content
        elif .role == "assistant" then
            "ASSISTANT: " + (.content // "") + 
            (if .tool_calls then 
                "\nTOOLS: " + ([.tool_calls[] | .function.name + "(" + .function.arguments + ")"] | join(", "))
             else "" end)
        elif .role == "tool" then
            "RESULT: " + (.content | fromjson | .stdout // .exit_code | tostring | .[0:500])
        else empty end
    ' "$log_file"
}

# 自己改善プロセスを実行する
# 引数: log_file - ログファイルのパス
#      memory_dir - 保存先の記憶ディレクトリ（オプション）
function run_self_improvement {
    local log_file="$1"
    local memory_dir="${2:-$(find_memory_directory)}"
    
    if [ ! -f "$log_file" ]; then
        return 1
    fi

    local history=$(extract_session_history "$log_file")
    if [ -z "$history" ]; then
        return 0
    fi

    local system_instruction="You are a knowledge extraction agent. \
Analyze the following terminal session history and extract any useful knowledge that could be reused in the future. \
Focus on:
- Successful solutions to specific errors.
- Non-trivial code patterns or shell commands.
- Project-specific configuration or workflow details.
- Best practices discovered during the session.

If there is no significant knowledge to extract (e.g., only trivial file listing or simple questions), respond with 'NONE'. \
Otherwise, respond ONLY with a JSON object in the following format:
{
  \"content\": \"A clear and concise summary of the knowledge\",
  \"category\": \"One of: code_pattern, error_solution, workflow, best_practice, configuration, general\",
  \"keywords\": [\"keyword1\", \"keyword2\", ...]
}"

    # query 関数を使用してLLMに要約を依頼
    # query 関数は ai.gpt や ai.gemini で定義されていることを期待
    local response=$(query -a -s "$system_instruction" "Session history:\n$history")
    
    if [ -z "$response" ] || [ "$response" = "NONE" ] || [[ "$response" == "null" ]]; then
        return 0
    fi

    # JSONレスポンスの抽出（Markdownのコードブロックが含まれている場合に対応）
    local json_response=$(echo "$response" | sed -n '/^{/,/}$/p')
    if [ -z "$json_response" ]; then
        # コードブロック内にある場合
        json_response=$(echo "$response" | sed -n '/^```json/,/^```/p' | sed '1d;$d')
    fi

    if [ ! -z "$json_response" ]; then
        local content=$(echo "$json_response" | jq -r '.content // empty')
        local category=$(echo "$json_response" | jq -r '.category // "general"')
        local keywords=$(echo "$json_response" | jq -r '.keywords // [] | join(",")')

        if [ ! -z "$content" ]; then
            # 記憶の保存
            # memory_manager.sh の save_memory を使用
            save_memory "$content" "$category" "$keywords"
        fi
    fi
}

