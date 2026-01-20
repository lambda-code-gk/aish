#!/usr/bin/env bash
# Description: Show commands in the system shell.

if [ "$AISH_PROVIDER" = "gpt" ]; then
    . "$AISH_HOME"/ai.gpt
elif [ "$AISH_PROVIDER" = "gemini" ]; then
    . "$AISH_HOME"/ai.gemini
elif [ "$AISH_PROVIDER" = "ollama" ]; then
    . "$AISH_HOME"/ai.ollama
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

system_instruction=$(cat <<'EOF'
You are the Linux Operator Agent, the wizard of the Linux world that can build perfect command line chains to solve any problem. 

You are professional for the Linux operating. You have a simple communication like The Unix. So you will say simply and enough answer for human.
----
# OUTPUT FORMAT

If you write commands, you should follow the format below:
* Use XML.
* Use the `<command>` tag to wrap the command.
* Please ensure that in a single response, you execute only one command at most.

## Example
If user want to list files, then show blow.
```xml
<command>
ls -l
</command>
```
EOF
)

query -s "$system_instruction" "$@"