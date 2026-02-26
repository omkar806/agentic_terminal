use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::Agent;
use crate::context::SharedContext;
use crate::display::Printer;
use crate::llm::LlmClient;
use crate::tools::{
    CheckProcessTool, KillProcessTool, ListDirectoryTool, ReadFileTool,
    ShellTool, SpawnAgentsTool, WriteFileTool,
};
use crate::tools::shell::RunScriptTool;

fn shell(working_dir: &PathBuf, sudo: bool) -> ShellTool {
    let tool = ShellTool::new(working_dir.clone());
    if sudo { tool.with_sudo() } else { tool }
}

pub fn file_agent(working_dir: &PathBuf, sudo: bool, interactive: bool) -> Agent {
    Agent::builder("FileAgent")
        .instructions(
            "You are a file operations specialist. You handle:\n\
             - Reading and writing files\n\
             - Listing directory contents\n\
             - Searching files with grep/find via shell\n\
             - File permission changes\n\
             Always confirm before overwriting existing files.\n\n\
             AUTONOMOUS SEARCH STRATEGY:\n\
             When searching for files/directories:\n\
             1. On macOS, try `mdfind \"kMDItemFSName == 'NAME'\"` first (instant Spotlight search)\n\
             2. If mdfind is unavailable, try `locate NAME` (if the locate database exists)\n\
             3. Fall back to `find` but start with likely directories (~, /Users, /home) before searching /\n\
             4. For `find /`, set timeout_secs to 300 or higher\n\
             5. Never give up after one timeout — try alternative approaches",
        )
        .tool(ReadFileTool)
        .tool(WriteFileTool::new(interactive))
        .tool(ListDirectoryTool)
        .tool(shell(working_dir, sudo))
        .max_turns(12)
        .build()
}

pub fn network_agent(working_dir: &PathBuf, sudo: bool) -> Agent {
    Agent::builder("NetworkAgent")
        .instructions(
            "You are a network operations specialist. You handle:\n\
             - HTTP requests (curl, wget)\n\
             - DNS lookups (dig, nslookup)\n\
             - Network connectivity checks (ping, traceroute)\n\
             - Port scanning and checking\n\
             - Downloading files\n\
             Use run_command for all network operations.\n\
             For downloads or slow endpoints, set timeout_secs appropriately.",
        )
        .tool(shell(working_dir, sudo))
        .max_turns(10)
        .build()
}

pub fn process_agent(working_dir: &PathBuf, sudo: bool) -> Agent {
    Agent::builder("ProcessAgent")
        .instructions(
            "You are a process management specialist. You handle:\n\
             - Listing running processes\n\
             - Checking process status\n\
             - Starting, stopping, and restarting services\n\
             - Managing cron jobs\n\
             - Monitoring resource usage\n\
             Always warn before killing processes.",
        )
        .tool(CheckProcessTool)
        .tool(KillProcessTool)
        .tool(shell(working_dir, sudo))
        .max_turns(10)
        .build()
}

pub fn package_agent(working_dir: &PathBuf, sudo: bool) -> Agent {
    Agent::builder("PackageAgent")
        .instructions(
            "You are a package management specialist. You handle:\n\
             - Installing packages (brew/apt/yum/pip/npm/cargo)\n\
             - Updating packages\n\
             - Removing packages\n\
             - Searching for packages\n\
             - Checking installed versions\n\
             Always use the appropriate package manager for the OS.\n\
             Use use_sudo=true when the package manager requires root privileges.\n\
             Use interactive=true for any command that may prompt the user for input \
             (e.g. npm init, brew install with cask prompts).",
        )
        .tool(shell(working_dir, sudo))
        .max_turns(10)
        .build()
}

pub fn code_agent(working_dir: &PathBuf, sudo: bool, interactive: bool) -> Agent {
    Agent::builder("CodeAgent")
        .instructions(
            "You are a code writing and execution specialist. You handle:\n\
             - Writing scripts in bash, python, node, ruby\n\
             - Executing scripts and capturing output\n\
             - Code analysis and debugging\n\
             - Building and compiling projects\n\
             Always validate script content before execution.\n\
             Use interactive=true for project scaffolding commands (npx create-*, cargo init, \
             rails new, etc.) and any command that presents selection menus or prompts.",
        )
        .tool(ReadFileTool)
        .tool(WriteFileTool::new(interactive))
        .tool(RunScriptTool {
            working_dir: working_dir.clone(),
        })
        .tool(shell(working_dir, sudo))
        .max_turns(12)
        .build()
}

/// Build the orchestrator agent with all specialist handoffs and spawn_agents tool.
/// `interactive`: if true, file writes show a diff preview and prompt for approval.
pub fn orchestrator(
    working_dir: &PathBuf,
    sudo: bool,
    interactive: bool,
    llm: Arc<LlmClient>,
    printer: Printer,
    context: SharedContext,
) -> Agent {
    let file = file_agent(working_dir, sudo, interactive);
    let network = network_agent(working_dir, sudo);
    let process = process_agent(working_dir, sudo);
    let package = package_agent(working_dir, sudo);
    let code = code_agent(working_dir, sudo, interactive);

    let sudo_note = if sudo {
        "You have SUDO ACCESS. Use use_sudo=true when a command needs root privileges."
    } else {
        "Sudo is DISABLED. If a command needs root, tell the user to restart with --sudo."
    };

    let instructions = format!(
        "You are the Agentic Terminal orchestrator — a fully autonomous terminal agent. \
         Your job is to COMPLETE tasks, not ask clarifying questions. When given a task:\n\
         1. Break it into independent subtasks\n\
         2. Execute directly or delegate to specialist agents via handoffs\n\
         3. If a command fails or times out, RETRY with a different strategy autonomously\n\
         4. Synthesize all results into a coherent final output\n\n\
         {sudo_note}\n\n\
         Available specialists:\n\
         - FileAgent: file reading, writing, listing, searching, permissions\n\
         - NetworkAgent: curl, wget, ping, DNS, port checks, downloads\n\
         - ProcessAgent: ps, kill, systemctl, cron, monitoring\n\
         - PackageAgent: brew/apt/pip/npm install (use sudo when needed)\n\
         - CodeAgent: write and execute scripts in multiple languages\n\n\
         PARALLEL EXECUTION:\n\
         When a task has 2+ INDEPENDENT subtasks, use the `spawn_agents` tool to run them \
         in parallel. Each subtask gets its own specialist agent. Available agent_types:\n\
           file, network, process, package, code, general\n\
         Example: to set up a project AND check ports simultaneously, spawn both at once.\n\
         Do NOT spawn for sequential/dependent tasks — just execute them in order.\n\
         After receiving spawn results, synthesize all outputs into your final response.\n\n\
         INTERACTIVE COMMANDS:\n\
         Some commands need user input (selection menus, y/n prompts, configuration wizards).\n\
         Set interactive=true on run_command for these. Examples:\n\
         - npx create-next-app, npx create-react-app, npx create-vite\n\
         - cargo init (when it asks questions), rails new, django-admin startproject\n\
         - Any installer or scaffolding tool that presents options\n\
         - Commands with y/n confirmation prompts\n\
         - npm init, yarn init, pip install with --user prompts\n\
         When interactive=true, the user sees the command output live and can respond.\n\
         You will only receive the exit code back (no stdout/stderr capture).\n\
         ALWAYS prefer interactive=true when you suspect a command may prompt the user.\n\n\
         AUTONOMOUS OPERATION RULES:\n\
         - NEVER give up after a single failure. Try at least 2-3 alternative approaches.\n\
         - If `find` times out, try: `mdfind` (macOS Spotlight), `locate`, or search narrower paths first.\n\
         - If a command fails, analyze stderr and fix the issue yourself.\n\
         - You can set timeout_secs on run_command (default 120s, max 600s) for long operations.\n\
         - For filesystem-wide searches, prefer: mdfind > locate > find with specific paths > find /\n\
         - Always try the fastest approach first.\n\
         - Be concise in your responses. Report what you found, not what you tried.",
    );

    let spawn_tool = SpawnAgentsTool::new(
        working_dir.clone(),
        sudo,
        interactive,
        llm,
        printer,
        context,
    );

    Agent::builder("Orchestrator")
        .instructions(&instructions)
        .tool(shell(working_dir, sudo))
        .tool(ReadFileTool)
        .tool(ListDirectoryTool)
        .tool(spawn_tool)
        .handoff(file)
        .handoff(network)
        .handoff(process)
        .handoff(package)
        .handoff(code)
        .max_turns(20)
        .build()
}
