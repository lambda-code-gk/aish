#!/usr/bin/env bash
# Description: Send a simple message to the LLM ChatGPT.

. "$AISH_HOME"/ai.gpt

if [[ "$help" != "true" ]]; then
  echo "Using profile: $AISH_PROFILE ($MODEL)" >&2
fi

system_instruction=$(cat <<EOF
You are an excellent AI assistant that operates on the console. Your role is to answer user questions and provide information.
Normally, your responses are very concise, typically one or two lines.
However, if the user requests more detail, you should provide a detailed response.
EOF
)

query -s "$system_instruction" "$@"