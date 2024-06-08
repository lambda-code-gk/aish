# AISH

The AISH is an assistant operator for Linux.

This command sends console input and output directly as requests to each API, so please be very careful not to inadvertently send requests of enormous size or sensitive information. Use it at your own risk.

This tool is still under development. Each function is incomplete.

# Installation

```bash
git clone https://github.com/lambda-code-gk/aish.git
cd aish
ln -s $PWD/_aish ~/.aish

if [ ! -d ~/bin ]; then
    mkdir ~/bin
fi
ln -s $PWD/ai ~/bin/ai
ln -s $PWD/aish ~/bin/aish
# If necessary, add the path to ~/bin to your PATH.

cat << EOF >> ~/.bashrc
if [ -n "\$AISH_SESSION" ]; then
    source ~/.aish/aishrc
fi
EOF
```

And set up the API key
```bash
export OPENAI_API_KEY=sk-...
```
or
```bash
export GOOGLE_API_KEY=...
```

## Prerequired

- jq
- curl


# Usage

Start the shell
```bash
$ aish
Script started, output log file is '/tmp/tmp.hbzjvInYGu/script.log'.
(aish:109)$ 
```
If you successfully start aish, you will see a prompt like (aish:109)$, where 109 represents the size of the history (not a token).

You can use a `ai` command to interact with the GPT-4o or Gemini 1.5 Pro.
```bash
(aish:109)$ ai "What is the meaning of life?"
```
or
```bash
(aish:109)$ ai gemini "What is the meaning of life?"
```

You can clear the user message by using the Ctrl+l key combination.

## Task

You can specify a task name as the first argument to the `ai` command. The following tasks are available:

- `ai default message`: Queries GPT-4. 
- `ai gemini message`: Queries Gemini 1.5 Pro.
- `ai op message`:  Generates shell scripts.
- `ai commit_msg`: Generates a commit message from the diff of files staged in git. 


# License
This project is licensed under the MIT License. See the LICENSE file for details.

