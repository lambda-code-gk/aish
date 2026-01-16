#!/usr/bin/env bash
# Description: Send a message to the LLM directly through Gemini.

. "$AISH_HOME"/ai.gemini

if [[ "$help" != "true" ]]; then
  echo "Using profile: $AISH_PROFILE ($MODEL)" >&2
fi

system_instruction="You are an AI assistant optimized for a Command-Line Interface (CUI). \
Keep your responses concise, to-the-point, and in plain text.\
Avoid verbose language, ASCII art, emojis, and any rich formatting."

query -s "$system_instruction" "$@"