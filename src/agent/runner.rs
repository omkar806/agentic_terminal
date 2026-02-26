use anyhow::Result;
use tracing::{info, warn};

use crate::agent::Agent;
use crate::context::budget;
use crate::context::SharedContext;
use crate::display::Printer;
use crate::llm::{
    ChatCompletionRequest, LlmClient, LlmResponseKind, Message,
};
use crate::memory::MemoryManager;
use crate::session::Session;

#[derive(Debug, Clone)]
pub struct RunResult {
    pub output: String,
    pub agent_name: String,
    pub turns_used: usize,
    /// True if the output was already printed via streaming
    pub streamed: bool,
}

pub struct Runner;

impl Runner {
    /// Core loop logic shared by both run variants.
    async fn agent_loop(
        agent: &Agent,
        messages: &mut Vec<Message>,
        context: &SharedContext,
        llm: &LlmClient,
        printer: &Printer,
        stream_output: bool,
    ) -> Result<(String, String, usize, bool)> {
        let mut current_agent = agent.clone();
        let mut turn = 0;

        loop {
            if turn >= current_agent.max_turns {
                warn!(
                    "Agent '{}' exceeded max_turns ({})",
                    current_agent.name, current_agent.max_turns
                );
                let msg = format!(
                    "[Agent '{}' stopped after {} turns — max_turns exceeded]",
                    current_agent.name, turn
                );
                return Ok((msg, current_agent.name.clone(), turn, false));
            }

            // Fit messages to the model's context window before each call.
            // This compresses old turns / tool results if we're approaching the limit.
            budget::fit_to_budget(messages, &current_agent.model, llm, printer).await;

            let tool_defs = current_agent.tool_definitions();

            let request = ChatCompletionRequest {
                model: current_agent.model.clone(),
                messages: messages.clone(),
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs)
                },
                tool_choice: None,
                temperature: Some(0.1),
                max_tokens: Some(4096),
            };

            printer.turn_start(&current_agent.name, turn + 1, current_agent.max_turns);

            let streamed = llm
                .chat_completion_stream(&request, stream_output)
                .await?;

            let handoff_names = current_agent.handoff_agent_names();
            let parsed = LlmClient::parse_streamed_response(&streamed, &handoff_names);

            match parsed {
                LlmResponseKind::FinalOutput(text) => {
                    let was_streamed = stream_output && !text.is_empty();
                    printer.final_output(&current_agent.name);

                    context
                        .log(
                            &current_agent.name,
                            "final_output",
                            &text.chars().take(200).collect::<String>(),
                        )
                        .await;

                    return Ok((text, current_agent.name.clone(), turn + 1, was_streamed));
                }

                LlmResponseKind::ToolCalls(calls) => {
                    info!(
                        "Agent '{}' made {} tool call(s)",
                        current_agent.name,
                        calls.len()
                    );

                    messages.push(Message::assistant_tool_calls(calls.clone()));

                    for call in &calls {
                        let tool_name = &call.function.name;
                        let args: serde_json::Value =
                            serde_json::from_str(&call.function.arguments)
                                .unwrap_or(serde_json::json!({}));

                        printer.tool_call(&current_agent.name, tool_name, &args);

                        context
                            .log(
                                &current_agent.name,
                                &format!("tool_call:{}", tool_name),
                                &call.function.arguments,
                            )
                            .await;

                        let result_str = if let Some(tool) = current_agent.find_tool(tool_name) {
                            match tool.execute(args).await {
                                Ok(val) => {
                                    printer.tool_result(tool_name, &val);
                                    let raw = serde_json::to_string(&val)
                                        .unwrap_or_else(|_| "{}".to_string());
                                    budget::truncate_result_string(&raw)
                                }
                                Err(e) => {
                                    let err_val = serde_json::json!({"error": e.to_string()});
                                    printer.tool_result(tool_name, &err_val);
                                    err_val.to_string()
                                }
                            }
                        } else {
                            let err_val = serde_json::json!({
                                "error": format!("Unknown tool: {}", tool_name)
                            });
                            printer.tool_result(tool_name, &err_val);
                            err_val.to_string()
                        };

                        messages.push(Message::tool_result(&call.id, &result_str));
                    }
                }

                LlmResponseKind::Handoff(target_name) => {
                    info!(
                        "Agent '{}' handing off to '{}'",
                        current_agent.name, target_name
                    );

                    context
                        .log(
                            &current_agent.name,
                            "handoff",
                            &format!("-> {}", target_name),
                        )
                        .await;

                    if let Some(target) = current_agent.find_handoff(&target_name) {
                        printer.handoff(&current_agent.name, &target.name);
                        if let Some(msg) = messages.first_mut() {
                            *msg = Message::system(&target.instructions);
                        }
                        current_agent = target.clone();
                    } else {
                        warn!("Handoff target '{}' not found", target_name);
                        messages.push(Message::user(&format!(
                            "Error: agent '{}' is not available. Please handle the request yourself.",
                            target_name
                        )));
                    }
                }
            }

            turn += 1;
        }
    }

    /// Run with memory manager (summarization-aware) + streaming.
    pub async fn run_with_memory(
        agent: &Agent,
        input: &str,
        memory: &MemoryManager,
        context: &SharedContext,
        llm: &LlmClient,
        printer: &Printer,
    ) -> Result<RunResult> {
        let mut messages = memory.build_context_messages().await?;
        messages.insert(0, Message::system(&agent.instructions));
        messages.push(Message::user(input));

        let (text, agent_name, turns, streamed) =
            Self::agent_loop(agent, &mut messages, context, llm, printer, true).await?;

        memory.add_user_message(input).await?;
        memory.add_assistant_message(&text).await?;
        memory.maybe_summarize(llm, printer).await?;

        if let Ok(stats) = memory.stats().await {
            printer.memory_stats(
                stats.total_messages,
                stats.unsummarized_messages,
                stats.summary_count,
                stats.next_summary_in,
            );
        }

        Ok(RunResult {
            output: text,
            agent_name,
            turns_used: turns,
            streamed,
        })
    }

    /// Run with a plain Session (no summarization — for sub-agents and one-shot tasks).
    pub async fn run(
        agent: &Agent,
        input: &str,
        session: &dyn Session,
        context: &SharedContext,
        llm: &LlmClient,
        printer: &Printer,
    ) -> Result<RunResult> {
        let mut messages = session.get_messages(None).await?;
        messages.insert(0, Message::system(&agent.instructions));
        messages.push(Message::user(input));

        let (text, agent_name, turns, streamed) =
            Self::agent_loop(agent, &mut messages, context, llm, printer, true).await?;

        session.add_message(Message::user(input)).await?;
        session.add_message(Message::assistant(&text)).await?;

        Ok(RunResult {
            output: text,
            agent_name,
            turns_used: turns,
            streamed,
        })
    }

    /// Run without streaming output — used by SpawnAgentsTool for sub-agents.
    pub async fn run_quiet(
        agent: &Agent,
        input: &str,
        session: &dyn Session,
        context: &SharedContext,
        llm: &LlmClient,
        printer: &Printer,
    ) -> Result<RunResult> {
        let mut messages = session.get_messages(None).await?;
        messages.insert(0, Message::system(&agent.instructions));
        messages.push(Message::user(input));

        let (text, agent_name, turns, _) =
            Self::agent_loop(agent, &mut messages, context, llm, printer, false).await?;

        Ok(RunResult {
            output: text,
            agent_name,
            turns_used: turns,
            streamed: false,
        })
    }

    pub fn run_sync(
        agent: &Agent,
        input: &str,
        session: &dyn Session,
        context: &SharedContext,
        llm: &LlmClient,
        printer: &Printer,
    ) -> Result<RunResult> {
        tokio::runtime::Runtime::new()?.block_on(Self::run(agent, input, session, context, llm, printer))
    }
}
