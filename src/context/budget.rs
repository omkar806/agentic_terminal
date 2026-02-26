use tracing::info;

use crate::display::Printer;
use crate::llm::{ChatCompletionRequest, LlmClient, Message};

const CHARS_PER_TOKEN: usize = 4;
const RESPONSE_RESERVE_TOKENS: usize = 8_000;
const TOOLS_RESERVE_TOKENS: usize = 2_000;
const MAX_SINGLE_RESULT_CHARS: usize = 40_000;
const AGGRESSIVE_TRUNCATE_CHARS: usize = 1_500;
const KEEP_RECENT_MESSAGES: usize = 8;

/// Get the context window size for a given model.
fn model_context_limit(model: &str) -> usize {
    if model.contains("gpt-4o") || model.contains("gpt-4-turbo") {
        128_000
    } else if model.contains("gpt-4") {
        8_192
    } else if model.contains("gpt-3.5") {
        16_385
    } else if model.contains("o1") || model.contains("o3") {
        200_000
    } else {
        128_000
    }
}

/// Rough token estimate for a slice of messages.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    let total_chars: usize = messages
        .iter()
        .map(|m| {
            let content_len = m.content.as_ref().map(|c| c.len()).unwrap_or(0);
            let tc_len = m
                .tool_calls
                .as_ref()
                .map(|tcs| {
                    tcs.iter()
                        .map(|tc| tc.function.name.len() + tc.function.arguments.len() + 20)
                        .sum::<usize>()
                })
                .unwrap_or(0);
            content_len + tc_len + 10
        })
        .sum();
    total_chars / CHARS_PER_TOKEN
}

/// Safety-net truncation for any individual tool result string.
pub fn truncate_result_string(s: &str) -> String {
    if s.len() <= MAX_SINGLE_RESULT_CHARS {
        return s.to_string();
    }
    let head = MAX_SINGLE_RESULT_CHARS * 2 / 3;
    let tail = MAX_SINGLE_RESULT_CHARS / 3;
    format!(
        "{}...\n[TRUNCATED: {} chars -> {} chars shown]\n...{}",
        &s[..head],
        s.len(),
        head + tail,
        &s[s.len() - tail..]
    )
}

/// Ensure messages fit within the model's context window.
/// Uses a multi-phase approach:
///   Phase 1: Truncate oversized tool results
///   Phase 2: Aggressively compress old tool results
///   Phase 3: Summarize old turns into a compact text block
///   Phase 4: LLM-powered summarization of old context (last resort)
pub async fn fit_to_budget(
    messages: &mut Vec<Message>,
    model: &str,
    llm: &LlmClient,
    printer: &Printer,
) {
    let budget = model_context_limit(model) - RESPONSE_RESERVE_TOKENS - TOOLS_RESERVE_TOKENS;
    let initial_tokens = estimate_tokens(messages);

    if initial_tokens <= budget {
        return;
    }

    info!(
        "Context over budget: ~{} tokens vs {} limit. Compacting...",
        initial_tokens, budget
    );

    // --- Phase 1: Truncate any oversized individual tool results ---
    for msg in messages.iter_mut() {
        if msg.role == "tool" {
            if let Some(ref content) = msg.content {
                if content.len() > MAX_SINGLE_RESULT_CHARS {
                    msg.content = Some(truncate_result_string(content));
                }
            }
        }
    }

    if estimate_tokens(messages) <= budget {
        printer.memory_event(
            "context trimmed",
            &format!("~{} -> ~{} tokens (truncated large results)", initial_tokens, estimate_tokens(messages)),
        );
        return;
    }

    // --- Phase 2: Aggressively compress ALL old tool results ---
    // Keep the last KEEP_RECENT_MESSAGES messages untouched
    let safe_end = messages.len().saturating_sub(KEEP_RECENT_MESSAGES);
    for msg in messages[..safe_end].iter_mut() {
        if msg.role == "tool" {
            if let Some(ref content) = msg.content {
                if content.len() > AGGRESSIVE_TRUNCATE_CHARS {
                    let short = &content[..AGGRESSIVE_TRUNCATE_CHARS.min(content.len())];
                    msg.content = Some(format!(
                        "{}...\n[Compressed: {} chars -> {}]",
                        short,
                        content.len(),
                        AGGRESSIVE_TRUNCATE_CHARS
                    ));
                }
            }
        }
        // Also compress old assistant messages with tool calls (the arguments)
        if let Some(ref mut tcs) = msg.tool_calls {
            for tc in tcs.iter_mut() {
                if tc.function.arguments.len() > 500 {
                    tc.function.arguments = format!(
                        "{}...",
                        &tc.function.arguments[..500]
                    );
                }
            }
        }
    }

    if estimate_tokens(messages) <= budget {
        printer.memory_event(
            "context compressed",
            &format!("~{} -> ~{} tokens (compressed old results)", initial_tokens, estimate_tokens(messages)),
        );
        return;
    }

    // --- Phase 3: Replace old turns with a text summary ---
    if messages.len() > KEEP_RECENT_MESSAGES + 1 {
        let system_msg = messages[0].clone();
        let old_msgs = &messages[1..messages.len() - KEEP_RECENT_MESSAGES];
        let summary = quick_text_summary(old_msgs);
        let recent: Vec<Message> = messages[messages.len() - KEEP_RECENT_MESSAGES..].to_vec();

        messages.clear();
        messages.push(system_msg);
        messages.push(Message::system(&format!(
            "## Compressed Context (earlier turns)\n\
             The following is a condensed summary of previous agent turns:\n\n{}",
            summary
        )));
        messages.extend(recent);

        let after = estimate_tokens(messages);
        printer.memory_event(
            "old turns summarized",
            &format!("~{} -> ~{} tokens", initial_tokens, after),
        );

        if after <= budget {
            return;
        }
    }

    // --- Phase 4: LLM-powered compression as last resort ---
    // Take the summary message and recent context, ask the LLM to compress
    let compressible: Vec<&Message> = messages
        .iter()
        .filter(|m| m.role == "system" && m.content.as_ref().map(|c| c.contains("Compressed Context")).unwrap_or(false))
        .collect();

    if let Some(to_compress) = compressible.first() {
        if let Some(ref content) = to_compress.content {
            let compress_req = ChatCompletionRequest {
                model: model.to_string(),
                messages: vec![
                    Message::system(
                        "You are a context compressor. Given the following conversation context, \
                         produce a much shorter version that preserves only the essential information: \
                         what was asked, what commands ran, key results, and current state. \
                         Be extremely concise. Use bullet points. Max 500 words.",
                    ),
                    Message::user(content),
                ],
                tools: None,
                tool_choice: None,
                temperature: Some(0.1),
                max_tokens: Some(1024),
            };

            if let Ok(response) = llm.chat_completion(&compress_req).await {
                if let Some(compressed) = response
                    .choices
                    .first()
                    .and_then(|c| c.message.content.clone())
                {
                    // Replace the compressed context message
                    for msg in messages.iter_mut() {
                        if msg.role == "system"
                            && msg.content.as_ref().map(|c| c.contains("Compressed Context")).unwrap_or(false)
                        {
                            msg.content = Some(format!(
                                "## Compressed Context (LLM summary)\n{}",
                                compressed
                            ));
                            break;
                        }
                    }

                    let final_tokens = estimate_tokens(messages);
                    printer.memory_event(
                        "LLM-compressed context",
                        &format!("~{} -> ~{} tokens", initial_tokens, final_tokens),
                    );
                }
            }
        }
    }
}

/// Quick text-based summary of messages without LLM call.
fn quick_text_summary(messages: &[Message]) -> String {
    let mut summary = String::new();
    let mut turn_num = 0;

    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                turn_num += 1;
                if let Some(ref c) = msg.content {
                    let short = if c.len() > 150 {
                        format!("{}...", &c[..150])
                    } else {
                        c.clone()
                    };
                    summary.push_str(&format!("Turn {}: User asked: {}\n", turn_num, short));
                }
            }
            "assistant" => {
                if let Some(ref c) = msg.content {
                    let short = if c.len() > 200 {
                        format!("{}...", &c[..200])
                    } else {
                        c.clone()
                    };
                    summary.push_str(&format!("  Agent replied: {}\n", short));
                }
                if let Some(ref tcs) = msg.tool_calls {
                    for tc in tcs {
                        let args_short = if tc.function.arguments.len() > 80 {
                            format!("{}...", &tc.function.arguments[..80])
                        } else {
                            tc.function.arguments.clone()
                        };
                        summary.push_str(&format!(
                            "  Called: {}({})\n",
                            tc.function.name, args_short
                        ));
                    }
                }
            }
            "tool" => {
                if let Some(ref c) = msg.content {
                    let short = if c.len() > 150 {
                        format!("{}...", &c[..150])
                    } else {
                        c.clone()
                    };
                    summary.push_str(&format!("  Result: {}\n", short));
                }
            }
            _ => {}
        }
    }

    // Cap the summary itself
    if summary.len() > 8000 {
        summary.truncate(8000);
        summary.push_str("\n[... earlier turns omitted ...]\n");
    }

    summary
}
