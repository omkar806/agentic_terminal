use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::task::JoinSet;
use tracing::info;

use crate::agent::{Runner, specialists};
use crate::context::SharedContext;
use crate::display::Printer;
use crate::llm::LlmClient;
use crate::session::InMemorySession;
use crate::tools::Tool;

const MAX_CONCURRENT_AGENTS: usize = 8;

pub struct SpawnAgentsTool {
    working_dir: PathBuf,
    sudo: bool,
    interactive: bool,
    llm: Arc<LlmClient>,
    printer: Printer,
    context: SharedContext,
}

impl std::fmt::Debug for SpawnAgentsTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnAgentsTool")
            .field("working_dir", &self.working_dir)
            .field("sudo", &self.sudo)
            .field("interactive", &self.interactive)
            .finish()
    }
}

impl SpawnAgentsTool {
    pub fn new(
        working_dir: PathBuf,
        sudo: bool,
        interactive: bool,
        llm: Arc<LlmClient>,
        printer: Printer,
        context: SharedContext,
    ) -> Self {
        Self {
            working_dir,
            sudo,
            interactive,
            llm,
            printer,
            context,
        }
    }
}

#[async_trait]
impl Tool for SpawnAgentsTool {
    fn name(&self) -> &str {
        "spawn_agents"
    }

    fn description(&self) -> &str {
        "Spawn multiple specialist sub-agents in parallel to execute independent subtasks concurrently. \
         Each sub-agent runs to completion and returns its result. Use this when a task can be decomposed \
         into 2+ independent subtasks that don't depend on each other."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "subtasks": {
                    "type": "array",
                    "description": "List of independent subtasks to execute in parallel",
                    "items": {
                        "type": "object",
                        "properties": {
                            "description": {
                                "type": "string",
                                "description": "Clear description of what the sub-agent should accomplish"
                            },
                            "agent_type": {
                                "type": "string",
                                "enum": ["file", "network", "process", "package", "code", "general"],
                                "description": "Which specialist agent to use. Defaults to 'general' if omitted.",
                                "default": "general"
                            }
                        },
                        "required": ["description"]
                    },
                    "minItems": 1,
                    "maxItems": MAX_CONCURRENT_AGENTS
                }
            },
            "required": ["subtasks"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let subtasks = args
            .get("subtasks")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'subtasks' array"))?;

        let count = subtasks.len().min(MAX_CONCURRENT_AGENTS);
        if count == 0 {
            return Ok(serde_json::json!({
                "error": "No subtasks provided"
            }));
        }

        eprintln!(
            "\n  \x1b[48;5;57m\x1b[97m\x1b[1m SPAWN \x1b[0m  \x1b[2mLaunching {} parallel agent(s)...\x1b[0m",
            count
        );

        let mut join_set = JoinSet::new();

        for (i, task_val) in subtasks.iter().take(count).enumerate() {
            let description = task_val
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("(no description)")
                .to_string();

            let agent_type = task_val
                .get("agent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("general")
                .to_string();

            let working_dir = self.working_dir.clone();
            let sudo = self.sudo;
            let interactive = self.interactive;
            let llm = Arc::clone(&self.llm);
            let printer = Printer::quiet();
            let context = self.context.clone();

            let agent_label = format!("Sub-{}-{}", agent_type, i + 1);

            eprintln!(
                "  \x1b[90m│\x1b[0m  \x1b[36m{}\x1b[0m \x1b[2m{}\x1b[0m",
                agent_label,
                if description.len() > 60 {
                    format!("{}...", &description[..57])
                } else {
                    description.clone()
                }
            );

            join_set.spawn(async move {
                let agent = build_specialist(&agent_type, &working_dir, sudo, interactive);
                let session = InMemorySession::new();

                let result = Runner::run_quiet(&agent, &description, &session, &context, &llm, &printer).await;

                context.store_result(&agent_label, &match &result {
                    Ok(r) => r.output.clone(),
                    Err(e) => format!("Error: {}", e),
                }).await;

                (description, agent.name.clone(), result)
            });
        }

        eprintln!("  \x1b[90m└───\x1b[0m \x1b[2mwaiting for all agents...\x1b[0m\n");

        let mut results = Vec::new();
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        let mut completed = 0usize;

        while let Some(res) = join_set.join_next().await {
            completed += 1;

            match res {
                Ok((desc, agent_name, Ok(run_result))) => {
                    succeeded += 1;
                    eprintln!(
                        "  \x1b[32m✓\x1b[0m \x1b[2m[{}/{}]\x1b[0m \x1b[1m{}\x1b[0m \x1b[2mcompleted ({} turns)\x1b[0m",
                        completed, count, agent_name, run_result.turns_used
                    );
                    results.push(serde_json::json!({
                        "subtask": desc,
                        "agent": agent_name,
                        "status": "success",
                        "output": run_result.output,
                        "turns_used": run_result.turns_used,
                    }));
                }
                Ok((desc, agent_name, Err(e))) => {
                    failed += 1;
                    eprintln!(
                        "  \x1b[31m✗\x1b[0m \x1b[2m[{}/{}]\x1b[0m \x1b[1m{}\x1b[0m \x1b[31mfailed: {}\x1b[0m",
                        completed, count, agent_name, e
                    );
                    results.push(serde_json::json!({
                        "subtask": desc,
                        "agent": agent_name,
                        "status": "error",
                        "output": e.to_string(),
                    }));
                }
                Err(e) => {
                    failed += 1;
                    eprintln!(
                        "  \x1b[31m✗\x1b[0m \x1b[2m[{}/{}]\x1b[0m \x1b[31mpanicked: {}\x1b[0m",
                        completed, count, e
                    );
                    results.push(serde_json::json!({
                        "subtask": "unknown",
                        "agent": "unknown",
                        "status": "error",
                        "output": format!("Agent panicked: {}", e),
                    }));
                }
            }
        }

        let summary = format!(
            "{} subtask(s): {} succeeded, {} failed",
            count, succeeded, failed
        );
        eprintln!(
            "\n  \x1b[48;5;57m\x1b[97m\x1b[1m SPAWN \x1b[0m  \x1b[2m{}\x1b[0m\n",
            summary
        );

        info!("spawn_agents completed: {}", summary);

        Ok(serde_json::json!({
            "results": results,
            "summary": summary,
            "total": count,
            "succeeded": succeeded,
            "failed": failed,
        }))
    }
}

fn build_specialist(agent_type: &str, working_dir: &PathBuf, sudo: bool, interactive: bool) -> crate::agent::Agent {
    match agent_type {
        "file" => specialists::file_agent(working_dir, sudo, interactive),
        "network" => specialists::network_agent(working_dir, sudo),
        "process" => specialists::process_agent(working_dir, sudo),
        "package" => specialists::package_agent(working_dir, sudo),
        "code" => specialists::code_agent(working_dir, sudo, interactive),
        _ => {
            // "general" — build an agent with all common tools
            crate::agent::Agent::builder("GeneralAgent")
                .instructions(&format!(
                    "You are a general-purpose terminal agent.\n\
                     Working directory: {}\n\
                     Execute the given task using available tools and return structured results.\n\
                     Be concise and precise. Report errors clearly.",
                    working_dir.display()
                ))
                .tool(crate::tools::ShellTool::new(working_dir.clone()))
                .tool(crate::tools::ReadFileTool)
                .tool(crate::tools::WriteFileTool::new(interactive))
                .tool(crate::tools::ListDirectoryTool)
                .max_turns(10)
                .build()
        }
    }
}
