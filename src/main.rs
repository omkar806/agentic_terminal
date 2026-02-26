#![allow(dead_code)]

mod agent;
mod cli;
mod context;
mod display;
mod llm;
mod memory;
mod safety;
mod session;
mod tools;

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use agent::{Runner, specialists};
use cli::{Cli, commands::Commands, detect};
use context::SharedContext;
use display::{Printer, markdown, printer::Verbosity};
use llm::LlmClient;
use memory::MemoryManager;
use safety::DryRunMode;
use session::{InMemorySession, Session, SqliteSession};

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .compact()
        .init();
}

fn resolve_api_key(cli_key: Option<&str>) -> anyhow::Result<String> {
    cli_key
        .map(|s| s.to_string())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No API key provided. Set OPENAI_API_KEY env var or pass --api-key"
            )
        })
}

fn resolve_workdir(workdir: Option<&str>) -> PathBuf {
    workdir
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn resolve_verbosity(verbose: bool, quiet: bool) -> Verbosity {
    if quiet {
        Verbosity::Quiet
    } else if verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    init_tracing();

    let cli = Cli::parse();
    let api_key = resolve_api_key(cli.api_key.as_deref())?;
    let llm = Arc::new(LlmClient::new(api_key));
    let printer = Printer::new(resolve_verbosity(cli.verbose, cli.quiet));

    match cli.command {
        Commands::Chat {
            sudo,
            session_id,
            workdir,
            db_path,
        } => {
            let mut working_dir = resolve_workdir(workdir.as_deref());
            let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let memory = MemoryManager::new(&sid, &db_path, &cli.model).await?;
            let context = SharedContext::new(&sid, working_dir.clone());

            let mut orchestrator = specialists::orchestrator(
                &working_dir, sudo, true,
                Arc::clone(&llm), printer.clone(), context.clone(),
            );

            printer.banner(
                env!("CARGO_PKG_VERSION"),
                &cli.model,
                &sid,
                &working_dir.display().to_string(),
                sudo,
            );

            eprintln!("  \x1b[2mTip: shell commands run directly. Use ? to force agent mode.\x1b[0m\n");

            let stdin = io::stdin();
            loop {
                // Show cwd-aware prompt
                let cwd_short = working_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| working_dir.display().to_string());
                print!("\x1b[36m{}\x1b[0m \x1b[1m>>\x1b[0m ", cwd_short);
                io::stdout().flush()?;

                let mut input = String::new();
                stdin.lock().read_line(&mut input)?;
                let input = input.trim();

                if input.is_empty() {
                    continue;
                }
                if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
                    println!("Session ended.");
                    break;
                }

                match detect::classify(input) {
                    detect::InputKind::Builtin(builtin) => {
                        match builtin {
                            detect::BuiltinCmd::Cd(path) => {
                                let target = if path == "~" || path == "~/" {
                                    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
                                } else if path.starts_with("~/") {
                                    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
                                    home.join(&path[2..])
                                } else if path.starts_with('/') {
                                    PathBuf::from(&path)
                                } else {
                                    working_dir.join(&path)
                                };

                                match target.canonicalize() {
                                    Ok(resolved) if resolved.is_dir() => {
                                        working_dir = resolved;
                                        orchestrator = specialists::orchestrator(
                                            &working_dir, sudo, true,
                                            Arc::clone(&llm), printer.clone(), context.clone(),
                                        );
                                    }
                                    Ok(_) => {
                                        eprintln!("\x1b[31mcd: not a directory: {}\x1b[0m", path);
                                    }
                                    Err(e) => {
                                        eprintln!("\x1b[31mcd: {}: {}\x1b[0m", path, e);
                                    }
                                }
                            }
                            detect::BuiltinCmd::Export(key, val) => {
                                // SAFETY: single-threaded CLI input loop, no concurrent env reads
                                unsafe { std::env::set_var(&key, &val); }
                                eprintln!("\x1b[2mexport {}={}\x1b[0m", key, val);
                            }
                            detect::BuiltinCmd::Clear => {
                                print!("\x1b[2J\x1b[H");
                                let _ = io::stdout().flush();
                            }
                        }
                    }

                    detect::InputKind::ShellCommand(cmd) => {
                        use tokio::process::Command;
                        use std::process::Stdio;

                        let status = Command::new("sh")
                            .arg("-c")
                            .arg(&cmd)
                            .current_dir(&working_dir)
                            .stdin(Stdio::inherit())
                            .stdout(Stdio::inherit())
                            .stderr(Stdio::inherit())
                            .status()
                            .await;

                        match status {
                            Ok(st) => {
                                let code = st.code().unwrap_or(-1);
                                if code != 0 {
                                    eprintln!("\x1b[90m[exit {}]\x1b[0m", code);
                                }
                            }
                            Err(e) => {
                                eprintln!("\x1b[31mFailed to execute: {}\x1b[0m", e);
                            }
                        }
                    }

                    detect::InputKind::AgentQuery(query) => {
                        match Runner::run_with_memory(&orchestrator, &query, &memory, &context, &llm, &printer).await {
                            Ok(result) => {
                                if !result.streamed {
                                    println!("\n  \x1b[48;5;27m\x1b[97m\x1b[1m ASSISTANT \x1b[0m\n");
                                    println!("{}", markdown::render_markdown(&result.output));
                                }
                                println!(
                                    "\n  \x1b[90m[Agent: {} | Turns: {}]\x1b[0m\n",
                                    result.agent_name, result.turns_used
                                );
                            }
                            Err(e) => {
                                eprintln!("\x1b[31mError: {}\x1b[0m\n", e);
                            }
                        }
                    }
                }
            }
        }

        Commands::Run {
            task,
            parallel: _,
            dry_run,
            sudo,
            workdir,
        } => {
            let working_dir = resolve_workdir(workdir.as_deref());
            let context = SharedContext::new("oneshot", working_dir.clone());
            let session = InMemorySession::new();

            if dry_run {
                DryRunMode::enable();
                println!("[DRY RUN MODE]\n");
            }

            let orchestrator = specialists::orchestrator(
                &working_dir, sudo, false,
                Arc::clone(&llm), printer.clone(), context.clone(),
            );

            println!("Task: {}\n", task);

            match Runner::run(&orchestrator, &task, &session, &context, &llm, &printer).await {
                Ok(result) => {
                    if !result.streamed {
                        println!("\n  \x1b[48;5;27m\x1b[97m\x1b[1m ASSISTANT \x1b[0m\n");
                        println!("{}", markdown::render_markdown(&result.output));
                    }
                    println!(
                        "\n  \x1b[90m[Agent: {} | Turns: {}]\x1b[0m",
                        result.agent_name, result.turns_used
                    );
                }
                Err(e) => {
                    eprintln!("\x1b[31mError: {}\x1b[0m", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Spawn { tasks, workdir } => {
            let working_dir = resolve_workdir(workdir.as_deref());
            let context = SharedContext::new("spawn", working_dir.clone());
            let tools_list: Vec<Arc<dyn tools::Tool>> = vec![
                Arc::new(tools::ShellTool::new(working_dir.clone())),
                Arc::new(tools::ReadFileTool),
                Arc::new(tools::WriteFileTool::new(false)),
                Arc::new(tools::ListDirectoryTool),
            ];

            println!("Spawning {} parallel agents...\n", tasks.len());

            let results = agent::parallel_execute(
                tasks,
                context,
                tools_list,
                cli.model,
                Arc::clone(&llm),
                printer,
            )
            .await;

            for r in &results {
                println!("--- {} ---", r.subtask);
                match &r.result {
                    Ok(res) => println!("{}\n", res.output),
                    Err(e) => println!("Error: {}\n", e),
                }
            }

            println!("[Completed {} tasks]", results.len());
        }

        Commands::History {
            session_id,
            db_path,
            limit,
        } => {
            let session = SqliteSession::new(&session_id, &db_path).await?;
            let messages = session.get_messages(limit).await?;

            if messages.is_empty() {
                println!("No messages found for session '{}'", session_id);
            } else {
                println!("Session: {} ({} messages)\n", session_id, messages.len());
                for msg in &messages {
                    let role = &msg.role;
                    let content = msg.content.as_deref().unwrap_or("[no content]");
                    let prefix = match role.as_str() {
                        "user" => ">>",
                        "assistant" => "<-",
                        "system" => "SYS",
                        "tool" => "TOOL",
                        _ => "?",
                    };
                    println!("[{}] {}", prefix, content);
                }
            }
        }

        Commands::Trace { session_id } => {
            println!(
                "Trace for session '{}' — (trace storage not yet implemented, \
                 use the logs in SharedContext during active sessions)",
                session_id
            );
        }
    }

    Ok(())
}
