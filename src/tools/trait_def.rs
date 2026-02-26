use async_trait::async_trait;
use serde_json::Value;
use std::fmt::Debug;

/// Core trait for all agent tools — the Rust equivalent of Python SDK's `@function_tool`.
///
/// Every tool exposes:
/// - A name used in LLM tool-call routing
/// - A description the LLM reads to decide when to invoke the tool
/// - A JSON Schema for parameter validation
/// - An async `execute` method that does the actual work
#[async_trait]
pub trait Tool: Send + Sync + Debug {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, args: Value) -> anyhow::Result<Value>;
}
