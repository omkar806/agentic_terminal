# Agentic Terminal (agterm)

**AI-powered shell orchestration in Rust.** A multi-agent terminal where an Orchestrator receives tasks, plans work, spawns specialist sub-agents, executes shell commands, and synthesizes results — all with shared context, memory, and optional sudo.

---

## What This Package Does

- **Single entry point:** You talk to one Orchestrator; it delegates to specialists (File, Network, Process, Package, Code) or runs tools itself.
- **Natural language + shell in one prompt:** Type a normal command (e.g. `ls`, `git status`) and it runs in your shell; type a question or task and the agent handles it.
- **Parallel sub-agents:** The Orchestrator can call `spawn_agents` to run multiple independent subtasks in parallel and then work with their combined results.
- **Session memory:** Conversations are stored; after a set number of messages, summaries are generated and used to keep context within model limits.
- **Streaming output:** Assistant replies stream token-by-token with markdown rendering (headers, code blocks, lists).
- **Interactive commands:** Commands that need user input (e.g. `npx create-next-app`, installers with prompts) can run with full terminal access so you see and answer prompts.
- **File-edit review:** In chat mode, file writes can show a diff-style preview with accept/reject (and optional line-by-line) before applying.
- **Safety:** Guardrails block dangerous patterns; optional dry-run and human confirmation for destructive operations; configurable sudo.

---

## Requirements

- **Rust** (latest stable; e.g. 1.70+)
- **OpenAI API key** (for the LLM used by agents)

---

## Installation

```bash
git clone <repo-url>
cd agentic_terminal
cargo build --release
```

Binary: `target/release/agterm`.

Or run without installing:

```bash
cargo run -- <command> [options]
```

---

## Configuration

### API key

Set one of:

- **Environment:** `export OPENAI_API_KEY=sk-...`
- **`.env` file:** In the project root, create `.env` with:
  ```bash
  OPENAI_API_KEY=sk-...
  ```
- **CLI:** `agterm --api-key sk-... <command> ...`

### Optional global flags (apply to any command)

| Flag | Description |
|------|-------------|
| `--api-key <KEY>` | OpenAI API key (overrides env) |
| `--model <NAME>` | LLM model (default: `gpt-5-mini`) |
| `-v, --verbose` | Detailed execution logs (tools, outputs) |
| `-q, --quiet` | Only final output; minimal logs |

---

## Commands

### 1. `chat` — Interactive session (main mode)

Start a long-lived session: mix natural-language tasks with direct shell commands. The prompt shows the current directory; `cd` changes it for subsequent commands.

```bash
agterm chat [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--sudo` | Allow agents to run commands with `sudo` when needed |
| `-s, --session-id <ID>` | Session ID for memory (default: new UUID) |
| `-w, --workdir <DIR>` | Working directory (default: current dir) |
| `--db-path <PATH>` | SQLite DB for session memory (default: `agterm_memory.db`) |

**Examples:**

```bash
agterm chat
agterm chat --sudo -w /home/me/projects
agterm chat -s my-session --db-path ./my_memory.db
```

**In the chat prompt:**

- **Shell commands** run directly (e.g. `ls`, `git status`, `npx create-next-app my-app`). Input is classified: known commands and things that “look like” shell (e.g. starting with `./`, or containing `|`, `>`, `&&`) run in the shell; everything else goes to the agent.
- **Override classification:**
  - `! <command>` — force shell (e.g. `! whoami`)
  - `? <question>` — force agent (e.g. `? who are you`)
- **Built-ins:** `cd <path>`, `export VAR=value`, `clear` are handled in-process (e.g. `cd` really changes the session working directory).
- **Exit:** Type `exit` or `quit`.

---

### 2. `run` — One-shot task

Run a single natural-language task and exit. No persistent session; no memory summarization.

```bash
agterm run <TASK> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--dry-run` | Print planned commands only; do not execute |
| `--sudo` | Enable sudo for shell tools |
| `-w, --workdir <DIR>` | Working directory |
| `--parallel` | (Reserved for future use) |

**Examples:**

```bash
agterm run "list all Rust files in this project and count lines"
agterm run "check if port 3000 is in use" -w /home/me/app
agterm run "summarize the README" --dry-run
```

---

### 3. `spawn` — Multiple parallel agents (CLI-driven)

Run several independent tasks in parallel, each with a generic agent (no orchestrator). Results are printed per task.

```bash
agterm spawn -t <TASK1> [-t <TASK2> ...] [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `-t, --tasks <TASK>[,<TASK>...]` | One or more task descriptions (comma-separated or repeated `-t`) |
| `-w, --workdir <DIR>` | Working directory |

**Examples:**

```bash
agterm spawn -t "list files in /tmp" -t "ping -c 3 8.8.8.8" -t "date"
agterm spawn -t "count lines in src" -t "check port 8080" -w ./myapp
```

---

### 4. `history` — Session message history

Print stored messages for a session (from the SQLite DB used by `chat`).

```bash
agterm history -s <SESSION_ID> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `-s, --session-id <ID>` | Session ID (required) |
| `--db-path <PATH>` | SQLite DB (default: `agterm_memory.db`) |
| `-l, --limit <N>` | Max number of messages to show |

**Examples:**

```bash
agterm history -s my-session
agterm history -s my-session -l 50 --db-path ./agterm_memory.db
```

---

### 5. `trace` — Execution trace

Placeholder for execution trace/logs by session. Currently prints a short message that trace storage is not yet implemented.

```bash
agterm trace -s <SESSION_ID>
```

---

## Supported Capabilities

### Agent hierarchy

- **Orchestrator:** Top-level agent in `chat` and `run`. Uses tools and handoffs to specialists; can call `spawn_agents` to run multiple sub-agents in parallel and then synthesize results.
- **Specialists (handoffs):**
  - **FileAgent** — read/write/list files, search (e.g. find, mdfind, locate), permissions.
  - **NetworkAgent** — curl, wget, ping, DNS, port checks, downloads.
  - **ProcessAgent** — processes (ps, kill), services, cron, monitoring.
  - **PackageAgent** — brew/apt/yum/pip/npm/cargo installs and updates; uses sudo when needed; uses interactive mode for prompts.
  - **CodeAgent** — write/run scripts (bash, python, node, ruby), code analysis, builds; uses interactive mode for scaffolding (e.g. npx create-*).

### Tools (what agents can call)

| Tool | Description |
|------|-------------|
| `run_command` | Execute a shell command. Options: `use_sudo`, `timeout_secs`, `interactive` (for prompts/wizards). |
| `read_file` | Read file contents (with truncation for large files). |
| `write_file` | Write content to a file. In chat mode can show diff and accept/reject. |
| `list_directory` | List directory contents with metadata. |
| `check_process` | Check if a process (by name) is running. |
| `kill_process` | Send a signal to a process. |
| `run_script` | Run a script (bash, python, node, ruby) from provided content. |
| `spawn_agents` | Run multiple subtasks in parallel with specialist agents; returns combined results. (Orchestrator only.) |

### Interactive commands

For commands that prompt the user (e.g. `npx create-next-app`, `npm init`, installers), the agent can call `run_command` with `interactive: true`. The command then runs with stdin/stdout/stderr connected to your terminal so you see prompts and can type answers. Only the exit code is reported back to the agent.

### Memory and context

- **Chat sessions** use SQLite (default: `agterm_memory.db`) to store messages.
- After a threshold of messages, a summary is generated and used to keep context within the model’s token limit.
- **Run** and **spawn** do not use this persistent memory.

### Safety

- Dangerous command patterns are blocked (e.g. `rm -rf /`, `mkfs`, etc.).
- **Dry-run** (`agterm run --dry-run ...`) prints commands without executing.
- Destructive or sensitive operations can require confirmation depending on configuration.
- **Sudo** is opt-in per session (`agterm chat --sudo` or `agterm run --sudo`).

---

## Project layout (high level)

```
agentic_terminal/
├── Cargo.toml
├── README.md
├── .env                    # optional; OPENAI_API_KEY
├── agterm_memory.db        # created by chat; session storage
└── src/
    ├── main.rs             # CLI entry, command dispatch
    ├── cli/                # CLI definition, command vs natural-language detection
    ├── agent/              # Agent, Runner, specialists, spawner
    ├── tools/              # Shell, filesystem, process, spawn_agents
    ├── llm/                # OpenAI API client, streaming
    ├── memory/             # Session store, summarization
    ├── context/            # Shared context, token budget
    ├── display/            # Terminal output, markdown, diff
    ├── safety/             # Guardrails, dry-run
    └── session/            # In-memory and SQLite session backends
```

---

## Quick reference: run this way

| Goal | Command |
|------|--------|
| Daily use (tasks + shell in one place) | `agterm chat` |
| One task and exit | `agterm run "your task"` |
| Several tasks in parallel (no orchestrator) | `agterm spawn -t "task1" -t "task2"` |
| Inspect past conversation | `agterm history -s <session-id>` |
| Use a specific model | `agterm --model gpt-4o chat` |
| Less noise | `agterm -q chat` |
| More detail | `agterm -v chat` |

---

## License

MIT.
