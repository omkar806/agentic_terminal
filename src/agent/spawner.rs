use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::info;

use crate::agent::{Agent, Runner, RunResult};
use crate::context::SharedContext;
use crate::display::Printer;
use crate::llm::LlmClient;
use crate::session::InMemorySession;
use crate::tools::Tool;

#[derive(Debug)]
pub struct SpawnResult {
    pub subtask: String,
    pub result: Result<RunResult, String>,
}

pub async fn spawn_agent_for_subtask(
    subtask: &str,
    context: &SharedContext,
    tools: Vec<Arc<dyn Tool>>,
    model: &str,
    llm: &LlmClient,
    printer: &Printer,
) -> anyhow::Result<RunResult> {
    let agent = Agent::builder(&format!("SubAgent-{}", &subtask[..subtask.len().min(20)]))
        .instructions(&format!(
            "You are a specialist terminal agent.\n\
             Working directory: {}\n\
             Task: {}\n\
             Execute the task using available tools and return structured results.\n\
             Be concise and precise. Report errors clearly.",
            context.working_dir.display(),
            subtask
        ))
        .model(model)
        .tools(tools)
        .max_turns(5)
        .build();

    let session = InMemorySession::new();
    Runner::run(&agent, subtask, &session, context, llm, printer).await
}

pub async fn parallel_execute(
    subtasks: Vec<String>,
    context: SharedContext,
    tools: Vec<Arc<dyn Tool>>,
    model: String,
    llm: Arc<LlmClient>,
    printer: Printer,
) -> Vec<SpawnResult> {
    info!("Spawning {} parallel agents", subtasks.len());

    let mut join_set = JoinSet::new();

    for subtask in subtasks.clone() {
        let ctx = context.clone();
        let t = tools.clone();
        let m = model.clone();
        let l = Arc::clone(&llm);
        let p = printer.clone();

        join_set.spawn(async move {
            let result = spawn_agent_for_subtask(&subtask, &ctx, t, &m, &l, &p).await;

            if let Ok(ref r) = result {
                ctx.store_result(&subtask, &r.output).await;
            }

            SpawnResult {
                subtask: subtask.clone(),
                result: result.map_err(|e| e.to_string()),
            }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(spawn_result) => results.push(spawn_result),
            Err(e) => results.push(SpawnResult {
                subtask: "unknown".to_string(),
                result: Err(format!("Task panicked: {}", e)),
            }),
        }
    }

    results
}
