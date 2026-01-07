# AISH

AISH is a CUI automation framework powered by LLMs, designed to supercharge your Linux command-line experience.
It integrates LLMs directly into your terminal, allowing you to interact with your shell using natural language, automate tasks, and even let an AI agent perform actions on your behalf.

⚠️ **Important**: AISH sends terminal input and output to external APIs (e.g., OpenAI, Google). Avoid transmitting large or sensitive data. Use at your own risk.

🚧 **Development Status**: AISH is under active development. Some features are experimental or incomplete. Feedback and contributions are welcome!

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/lambda-code-gk/aish)

## ✨ Features

*   **LLM-embedded environment**: AISH integrates large language models (GPT, Gemini) directly into your terminal session.
*   **Context-aware interactions**: The `ai` command captures your terminal session's context, allowing the LLM to understand what you're working on.
*   **AI Agent (New!)**: The `ai agent` task allows the LLM to execute shell commands to accomplish complex tasks autonomously.
*   **Task-oriented workflows**: Specialized tasks for code review, commit message generation, bug fixing, and more.
*   **High-performance tools**: Core components like terminal capture and script execution are implemented in Rust for efficiency and reliability.

## 🚀 Quick Start

### Requirements

- **Rust & Cargo**: Required to build the core tools.
- **Python 3.8+**: Required for some LLM interaction scripts.
- **Dependencies**: `jq`, `curl`, `script` (standard on most Linux systems).

### Installation

1.  **Clone the repository**:
    ```bash
    git clone https://github.com/lambda-code-gk/aish.git
    cd aish
    ```

2.  **Build the core tools**:
    ```bash
    ./build.sh
    ```

3.  **Setup symlinks**:
    ```bash
    # Link configuration
    ln -s $PWD/_aish ~/.aish

    # Link binaries to your path (e.g., ~/bin)
    mkdir -p ~/bin
    ln -s $PWD/ai ~/bin/ai
    ln -s $PWD/aish ~/bin/aish
    # Ensure ~/bin is in your PATH
    ```

4.  **Configure your shell**:
    Add the following to your `~/.bashrc`:
    ```bash
    if [ -n "$AISH_SESSION" ]; then
        source ~/.aish/aishrc
    fi
    ```

5.  **Set your API keys**:
    Create or update `~/.apikey`:
    ```bash
    export OPENAI_API_KEY=sk-...
    export GOOGLE_API_KEY=...
    ```

### Launching AISH

To start an AISH session, simply run:
```bash
$ aish
(aish:0)$
```

The prompt shows `(aish:N)` where `N` is the current size of the session context in tokens (estimated).

## 🛠 Available Tasks

You can use the `ai <task>` command to perform various actions.

| Task | Description |
| :--- | :--- |
| `ai agent` | **Agent mode**: Execute tasks using function calling with shell command execution. |
| `ai review` | Review code changes staged in Git. |
| `ai commit_msg` | Generate a Git commit message based on staged changes. |
| `ai fixit` | Get AI advice on how to fix errors or improve code. |
| `ai op` | Translate natural language to shell commands. |
| `ai gpt` / `ai gemini` | Direct interaction with specific LLMs. |
| `ai default` | General purpose chat with context. |

Example:
```bash
(aish:120)$ ai agent "Update the README.md to include information about the new Rust tools"
```

## 🧰 Core Tools (Rust)

AISH includes several high-performance tools written in Rust:

*   **`aish-capture`**: A lightweight PTY capture tool that records terminal sessions into JSONL format.
*   **`aish-render`**: A tool to process and render terminal logs for LLM consumption.
*   **`aish-script`**: An expect-like script execution tool used for automated interactions and testing.

## 🧭 Roadmap & Future Plans

*   Advanced session management and history context.
*   Support for more LLM providers and local models.
*   Enhanced agent capabilities and safety controls.

## 📄 License

This project is licensed under the MIT License. See the LICENSE file for details.
