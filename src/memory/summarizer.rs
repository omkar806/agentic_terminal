use tracing::info;

use crate::llm::{ChatCompletionRequest, LlmClient, Message};

const SUMMARIZE_PROMPT: &str = "\
You are a conversation summarizer for an AI terminal agent. \
Given the conversation history below, produce a concise summary that captures:\n\
1. What tasks the user asked for\n\
2. What commands were executed and their key results\n\
3. Any important decisions, errors, or outcomes\n\
4. Current state of the system/environment if relevant\n\n\
Be factual and concise. Use bullet points. \
Do NOT include greetings or filler. Focus on actionable context \
that would help a future agent continue the conversation seamlessly.";

/// Use the LLM to compress a batch of messages into a summary paragraph.
pub async fn summarize_messages(
    messages: &[Message],
    llm: &LlmClient,
    model: &str,
) -> anyhow::Result<String> {
    let mut conversation_text = String::new();
    for msg in messages {
        let role_label = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "tool" => "Tool Result",
            "system" => continue,
            _ => &msg.role,
        };

        if let Some(ref content) = msg.content {
            let truncated = if content.len() > 500 {
                format!("{}...", &content[..500])
            } else {
                content.clone()
            };
            conversation_text.push_str(&format!("{}: {}\n", role_label, truncated));
        }

        if let Some(ref tool_calls) = msg.tool_calls {
            for tc in tool_calls {
                conversation_text.push_str(&format!(
                    "Assistant called tool: {} with args: {}\n",
                    tc.function.name,
                    if tc.function.arguments.len() > 200 {
                        format!("{}...", &tc.function.arguments[..200])
                    } else {
                        tc.function.arguments.clone()
                    }
                ));
            }
        }
    }

    let request = ChatCompletionRequest {
        model: model.to_string(),
        messages: vec![
            Message::system(SUMMARIZE_PROMPT),
            Message::user(&format!(
                "Summarize this conversation ({} messages):\n\n{}",
                messages.len(),
                conversation_text
            )),
        ],
        tools: None,
        tool_choice: None,
        temperature: Some(0.2),
        max_tokens: Some(1024),
    };

    info!("Generating summary for {} messages...", messages.len());

    let response = llm.chat_completion(&request).await?;
    let summary = response
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_else(|| "[Summary generation failed]".to_string());

    info!("Summary generated: {} chars", summary.len());

    Ok(summary)
}
