#!/usr/bin/env bash
# Description: Create a commit message for the staged files in Git.

if [ "$AISH_PROVIDER" = "gpt" ]; then
    . "$AISH_HOME"/ai.gpt
elif [ "$AISH_PROVIDER" = "gemini" ]; then
    . "$AISH_HOME"/ai.gemini
else
    # Fallback to legacy behavior
    if [ "$MODEL" = "gpt" ]; then
        . "$AISH_HOME"/ai.gpt
    else
        . "$AISH_HOME"/ai.gemini
    fi
fi

if [[ "$help" != "true" ]]; then
  echo "Using profile: $AISH_PROFILE ($MODEL)" >&2
fi

echo ----
echo "# log"
git --no-pager log -n 5
echo ----
echo "# diff"
git --no-pager diff --cached
echo ----
query "上記のdiffから意図を抽出しコミットメッセージを書いて下さい。"

ai_generated_message=$(ls "$AISH_PART"/part_*_assistant.txt | tail -n 1)

commit_message_file="$AISH_SESSION"/commit_message.txt
echo "This is an auto-generated commit message" > "$commit_message_file"
echo "If you want to commit, please delete this comment" >> "$commit_message_file"
echo "####" >> "$commit_message_file"
cat "$ai_generated_message" >> "$commit_message_file"
echo "commit message: $commit_message_file"
git commit --template="$commit_message_file"