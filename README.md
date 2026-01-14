# AISH

AISH is a CUI automation framework powered by LLMs, designed to supercharge your Linux command-line experience.
It integrates LLMs directly into your terminal, allowing you to interact with your shell using natural language, automate tasks, and let an AI agent perform actions on your behalf with full context awareness.

⚠️ **Important**: AISH sends terminal input and output to external APIs (e.g., OpenAI, Google). Avoid transmitting large or sensitive data. Use at your own risk.

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/lambda-code-gk/aish)

## ✨ Features

*   **LLM-embedded environment**: AISH integrates large language models (GPT-4, Gemini 1.5 Pro/Flash) directly into your terminal session.
*   **Context-aware interactions**: The `ai` command captures your terminal session's context (inputs and outputs), allowing the LLM to understand exactly what you're working on.
*   **AI Agent (Advanced!)**: The `ai agent` task features a sophisticated PLAN/TOOL_CALL/OBSERVE/ANSWER loop, enabling the LLM to execute shell commands, manage files, and search memory autonomously.
*   **Memory System**: A persistent "Search then Fetch" memory architecture. The agent can store (`save_memory`), search (`search_memory`), and retrieve past successes, code patterns, and project-specific knowledge. It supports multi-ID operations and efficient context injection.
*   **Security Scanning**: Integrated high-performance `leakscan` (Rust) that uses keyword filtering, regex, and Shannon entropy to prevent accidental transmission of sensitive data.
*   **Task-oriented workflows**: Specialized tasks for code review, commit message generation, bug fixing, and more.

## 🚀 Quick Start

### Requirements

- **Rust & Cargo**: Required to build the core high-performance tools.
- **Python 3.8+**: Required for LLM interaction scripts.
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
    # Link configuration directory to your home
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
    Create or update `~/.apikey` (or set them as environment variables):
    ```bash
    export OPENAI_API_KEY=sk-...
    export GOOGLE_API_KEY=...
    ```

### Launching AISH

To start a new AISH session, run:
```bash
$ aish
(aish:0)$
```

The prompt shows `(aish:N)` where `N` is the current size of the session context in tokens (e.g., `(aish:1.2K)$`).

### Session Management

AISH now supports persistent sessions, allowing you to resume your work later.

*   **List sessions**: `aish sessions`
*   **Resume latest session**: `aish resume`
*   **Resume specific session**: `aish resume [session_id]`

Sessions are stored in `$AISH_HOME/sessions/`.

## 🛠 Available Tasks

You can use the `ai <task>` command to perform various actions.

| Task | Description |
| :--- | :--- |
| `ai agent` | **Agent mode**: Execute complex tasks using tool calling (shell, memory, file IO). |
| `ai review` | Review code changes staged in Git. |
| `ai commit_msg` | Generate a Git commit message based on staged changes. |
| `ai fixit` | Get AI advice on how to fix errors or improve code. |
| `ai op` | Translate natural language to shell commands and execute them. |
| `ai editor` | Tasks related to code editing and file manipulation. |
| `ai gpt` / `ai gemini` | Direct interaction with specific LLM providers. |
| `ai default` | General purpose chat with full terminal context. |

## 🧰 Core Tools (Rust)

AISH includes several high-performance tools written in Rust for efficiency and reliability:

*   **`leakscan`**: A high-performance sensitivity detection engine used to prevent accidental transmission of secrets.
*   **`aish-capture`**: A lightweight PTY capture tool that records terminal sessions into JSONL format.
*   **`aish-render`**: A tool to process and render terminal logs for LLM consumption. It includes a `-f` / `--follow` mode for real-time monitoring.

## 📂 Project Structure

```
aish/
├── ai                  # Main 'ai' task runner
├── aish                # Session entry point
├── _aish/              # Configuration and library directory
│   ├── aishrc          # Shell initialization
│   ├── functions       # Core shell function library
│   ├── lib/            # Logic for Agent and Memory systems
│   ├── bin/            # Compiled Rust binaries
│   ├── memory/         # Persistent knowledge storage
│   ├── sessions/       # Persistent session logs and state
│   ├── task.d/         # Task definitions (review, agent, etc.)
│   └── rules.json      # Leak detection rules
├── tools/              # Source code for Rust tools (leakscan, etc.)
└── devel/              # Development and testing scripts
```

## 🧭 Roadmap & Future Plans

*   **Self-Improvement**: Agent cycles to optimize its own tools and memory.
*   **Context Optimization**: Automatic summarization of long terminal logs to save tokens.
*   **Visual Understanding**: Support for terminal visual context (screenshots/terminal state).

## 📄 License

This project is licensed under the MIT License. See the LICENSE file for details.
