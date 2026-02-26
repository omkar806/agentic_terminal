use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agterm",
    about = "Agentic Terminal — AI-powered shell orchestration in Rust",
    version,
    long_about = "A multi-agent terminal system where an Orchestrator Agent receives tasks, \
                   plans work, spawns specialist sub-agents, executes shell commands, \
                   and synthesizes results — all with shared context."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// OpenAI API key (can also be set via OPENAI_API_KEY env var)
    #[arg(long, global = true, env = "OPENAI_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// LLM model to use
    #[arg(long, global = true, default_value = "gpt-5-mini")]
    pub model: String,

    /// Show detailed execution logs (commands, outputs, file contents)
    #[arg(short, long, global = true, default_value_t = false)]
    pub verbose: bool,

    /// Suppress execution logs, only show final output
    #[arg(short, long, global = true, default_value_t = false)]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start an interactive agent chat session
    Chat {
        /// Enable sudo for shell commands
        #[arg(long, default_value_t = false)]
        sudo: bool,

        /// Session ID for persistent memory (auto-generated if not provided)
        #[arg(short, long)]
        session_id: Option<String>,

        /// Working directory for commands
        #[arg(short, long)]
        workdir: Option<String>,

        /// SQLite database path for session persistence
        #[arg(long, default_value = "agterm_memory.db")]
        db_path: String,
    },

    /// Run a one-shot task and exit
    Run {
        /// The task to execute
        task: String,

        /// Enable parallel agent spawning for subtasks
        #[arg(long, default_value_t = false)]
        parallel: bool,

        /// Dry-run mode: print commands without executing
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Enable sudo for shell commands
        #[arg(long, default_value_t = false)]
        sudo: bool,

        /// Working directory for commands
        #[arg(short, long)]
        workdir: Option<String>,
    },

    /// Spawn multiple agents for parallel subtasks
    Spawn {
        /// Subtask descriptions (comma-separated or multiple flags)
        #[arg(short, long, value_delimiter = ',')]
        tasks: Vec<String>,

        /// Working directory for commands
        #[arg(short, long)]
        workdir: Option<String>,
    },

    /// Show session history
    History {
        /// Session ID to show history for
        #[arg(short, long)]
        session_id: String,

        /// SQLite database path
        #[arg(long, default_value = "agterm_memory.db")]
        db_path: String,

        /// Number of messages to show
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Show execution trace/logs
    Trace {
        /// Session ID to show trace for
        #[arg(short, long)]
        session_id: String,
    },
}
