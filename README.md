# AISH

AISH is a CUI automation framework powered by LLMs, designed to supercharge your Linux command-line experience.
It lets you interact with your terminal using natural language — generate shell commands, review code, write commit messages, and fix bugs, all without switching context.

⚠️ Important: AISH sends terminal input and output to external APIs (e.g., OpenAI, Google). Avoid transmitting large or sensitive data. Use at your own risk.

🚧 Development Status: AISH is under active development. Some features are experimental or incomplete. Feedback and contributions are welcome!

## ✨ Features

* **LLM-embedded environment for command-line workflows**<br>
  AISH integrates large language models directly into your terminal session, making AI a first-class part of your CUI experience.

* **Natural language interface for the terminal**<br>
  AISH allows you to interact with GPT or Gemini directly from your shell using the `aish` and `ai` command. It captures standard output and sends it to the model, providing contextual awareness of your terminal session.

* **Task-oriented commands**<br>
  AISH provides specialized commands for common tasks like code review, commit message generation, and bug fixing. Just type `ai <task>` to get started.


## 🚀 Quick Start

### Requirements

- jq
- curl
- Python 3.8 or later

### Installation

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

Then set your API key:

~/.apikey
```
export OPENAI_API_KEY=sk-...
export GOOGLE_API_KEY=...
```

### Launching AISH

To start the shell, run the following command:
```bash
$ aish
Script started, output log file is '/tmp/tmp.xxxxxxxxxx/script.log'.
(aish:109)$ 
```

You can use the `ai` command to interact with the LLM.
```bash
(aish:0)$ cat README.md
....
(aish:109)$ ai "TL;DR"
```

You can clear the user message by using the Ctrl+L key combination.

## 🛠 Available Tasks

You can specify a task name as the first argument to the `ai` command. The following tasks are available:

- `ai default`:     Send a simple message to the LLM.
- `ai gemini`:      Send a message to the LLM directly through Gemini.
- `ai review`:      Review the code and give feedback on the files staged for Git.
- `ai commit_msg`:  Create a commit message for the staged files in Git.
- `ai fixit`:       Show the advice from the LLM on how to fix the code.


You can also append additional arguments to the `ai` command to provide more context or specify options for the task. For example:
```bash
(aish:0)$ ai review "Feedback only architecture and design, no code style"
```


## 🧭 Roadmap & Future Plans

* Session management and history context
* Improve shell script generation and execution
* Pro features

We aim to keep the core open-source. Advanced features may become part of a Pro tier in the future.

## 📄 License
This project is licensed under the MIT License. See the LICENSE file for details.

