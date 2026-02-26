use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::Client;
use tracing::{debug, info};

use super::types::*;
use crate::display::markdown;

pub struct LlmClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl LlmClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".into(),
        }
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.trim_end_matches('/').to_string();
        self
    }

    pub fn build_tools(tools: &[&dyn crate::tools::Tool]) -> Vec<ToolDefinition> {
        tools
            .iter()
            .map(|t| ToolDefinition {
                tool_type: "function".into(),
                function: FunctionDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters_schema(),
                },
            })
            .collect()
    }

    /// Non-streaming completion (used for summarization, sub-agents, etc.)
    pub async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        debug!("LLM request to {} with model {}", url, request.model);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .context("Failed to send request to LLM API")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        if !status.is_success() {
            if let Ok(api_err) = serde_json::from_str::<ApiError>(&body) {
                anyhow::bail!(
                    "LLM API error ({}): {}",
                    status,
                    api_err.error.message
                );
            }
            anyhow::bail!("LLM API error ({}): {}", status, body);
        }

        let parsed: ChatCompletionResponse =
            serde_json::from_str(&body).context("Failed to parse LLM response")?;

        info!(
            "LLM response: {} tokens used",
            parsed.usage.as_ref().map_or(0, |u| u.total_tokens)
        );

        Ok(parsed)
    }

    /// Streaming completion — renders text output line-by-line with markdown formatting.
    /// Tool call deltas are accumulated silently. Returns the full assembled response.
    pub async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
        render_output: bool,
    ) -> Result<StreamedResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let stream_req = StreamingRequest::from_request(request);

        debug!("LLM streaming request to {} with model {}", url, request.model);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&stream_req)
            .send()
            .await
            .context("Failed to send streaming request to LLM API")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if let Ok(api_err) = serde_json::from_str::<ApiError>(&body) {
                anyhow::bail!(
                    "LLM API error ({}): {}",
                    status,
                    api_err.error.message
                );
            }
            anyhow::bail!("LLM API error ({}): {}", status, body);
        }

        let mut content_buf = String::new();
        let mut tool_calls_map: std::collections::HashMap<u32, (String, String, String)> =
            std::collections::HashMap::new();
        let mut finish_reason = None;
        let mut has_content = false;
        let mut printed_header = false;

        // Markdown rendering state
        let mut md_line_buf = String::new();
        let mut in_code_block = false;

        let mut byte_stream = response.bytes_stream();
        let mut sse_buf = String::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let bytes = chunk_result.context("Error reading stream chunk")?;
            let text = String::from_utf8_lossy(&bytes);

            sse_buf.push_str(&text);

            while let Some(pos) = sse_buf.find('\n') {
                let line = sse_buf[..pos].trim().to_string();
                sse_buf = sse_buf[pos + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                let data = if let Some(stripped) = line.strip_prefix("data: ") {
                    stripped.trim()
                } else {
                    continue;
                };

                if data == "[DONE]" {
                    continue;
                }

                let chunk: StreamChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                for choice in &chunk.choices {
                    if let Some(ref reason) = choice.finish_reason {
                        finish_reason = Some(reason.clone());
                    }

                    if let Some(ref token) = choice.delta.content {
                        content_buf.push_str(token);

                        if render_output {
                            // Print ASSISTANT header on first content token
                            if !printed_header {
                                printed_header = true;
                                has_content = true;
                                eprint!("\n  \x1b[48;5;27m\x1b[97m\x1b[1m ASSISTANT \x1b[0m\n\n");
                            }

                            md_line_buf.push_str(token);

                            // Render completed lines through markdown renderer
                            while let Some(nl) = md_line_buf.find('\n') {
                                let completed = md_line_buf[..nl].to_string();
                                let rendered = markdown::render_line(&completed, &mut in_code_block);
                                println!("{}", rendered);
                                md_line_buf = md_line_buf[nl + 1..].to_string();
                            }
                        } else {
                            has_content = true;
                        }
                    }

                    if let Some(ref tc_deltas) = choice.delta.tool_calls {
                        for tc in tc_deltas {
                            let entry = tool_calls_map
                                .entry(tc.index)
                                .or_insert_with(|| (String::new(), String::new(), String::new()));

                            if let Some(ref id) = tc.id {
                                entry.0 = id.clone();
                            }
                            if let Some(ref func) = tc.function {
                                if let Some(ref name) = func.name {
                                    entry.1.push_str(name);
                                }
                                if let Some(ref args) = func.arguments {
                                    entry.2.push_str(args);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Flush any remaining partial line
        if render_output && !md_line_buf.is_empty() {
            let rendered = markdown::render_line(&md_line_buf, &mut in_code_block);
            println!("{}", rendered);
        }

        if has_content && render_output {
            println!();
        }

        let mut tool_calls: Vec<ToolCallResponse> = tool_calls_map
            .into_iter()
            .map(|(_, (id, name, args))| ToolCallResponse {
                id,
                call_type: "function".into(),
                function: FunctionCallResponse {
                    name,
                    arguments: args,
                },
            })
            .collect();
        tool_calls.sort_by_key(|tc| tc.id.clone());

        let content = if content_buf.is_empty() {
            None
        } else {
            Some(content_buf)
        };

        Ok(StreamedResponse {
            content,
            tool_calls,
            finish_reason,
        })
    }

    /// Parse a non-streaming response into a typed result.
    pub fn parse_response(
        response: &ChatCompletionResponse,
        handoff_agent_names: &[String],
    ) -> Result<LlmResponseKind> {
        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow::anyhow!("No choices in LLM response"))?;

        if let Some(ref tool_calls) = choice.message.tool_calls {
            if !tool_calls.is_empty() {
                for tc in tool_calls {
                    let fn_name = &tc.function.name;
                    if fn_name.starts_with("transfer_to_") {
                        let agent_name = fn_name.strip_prefix("transfer_to_").unwrap();
                        if handoff_agent_names.iter().any(|n| {
                            n.to_lowercase().replace(' ', "_") == agent_name.to_lowercase()
                        }) {
                            return Ok(LlmResponseKind::Handoff(agent_name.to_string()));
                        }
                    }
                }
                return Ok(LlmResponseKind::ToolCalls(tool_calls.clone()));
            }
        }

        let content = choice
            .message
            .content
            .clone()
            .unwrap_or_default();

        Ok(LlmResponseKind::FinalOutput(content))
    }

    /// Parse a streamed response into the same typed result.
    pub fn parse_streamed_response(
        response: &StreamedResponse,
        handoff_agent_names: &[String],
    ) -> LlmResponseKind {
        if !response.tool_calls.is_empty() {
            for tc in &response.tool_calls {
                let fn_name = &tc.function.name;
                if fn_name.starts_with("transfer_to_") {
                    let agent_name = fn_name.strip_prefix("transfer_to_").unwrap();
                    if handoff_agent_names.iter().any(|n| {
                        n.to_lowercase().replace(' ', "_") == agent_name.to_lowercase()
                    }) {
                        return LlmResponseKind::Handoff(agent_name.to_string());
                    }
                }
            }
            return LlmResponseKind::ToolCalls(response.tool_calls.clone());
        }

        let content = response.content.clone().unwrap_or_default();
        LlmResponseKind::FinalOutput(content)
    }
}
