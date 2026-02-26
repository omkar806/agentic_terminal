use std::sync::Arc;
use crate::tools::Tool;
use crate::llm::ToolDefinition;

/// Core Agent struct — equivalent to the Python SDK's Agent class.
/// Contains model config, instructions, tools, and handoff targets.
#[derive(Clone)]
pub struct Agent {
    pub name: String,
    pub instructions: String,
    pub model: String,
    pub tools: Vec<Arc<dyn Tool>>,
    pub handoffs: Vec<Agent>,
    pub max_turns: usize,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("name", &self.name)
            .field("model", &self.model)
            .field("tools", &self.tools.iter().map(|t| t.name()).collect::<Vec<_>>())
            .field("handoffs", &self.handoffs.iter().map(|a| &a.name).collect::<Vec<_>>())
            .field("max_turns", &self.max_turns)
            .finish()
    }
}

impl Agent {
    pub fn builder(name: &str) -> AgentBuilder {
        AgentBuilder::new(name)
    }

    /// Get the tool definitions for the LLM API, including handoff "transfer_to_*" functions.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<ToolDefinition> = self
            .tools
            .iter()
            .map(|t| ToolDefinition {
                tool_type: "function".into(),
                function: crate::llm::FunctionDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters_schema(),
                },
            })
            .collect();

        for handoff in &self.handoffs {
            let fn_name = format!(
                "transfer_to_{}",
                handoff.name.to_lowercase().replace(' ', "_")
            );
            defs.push(ToolDefinition {
                tool_type: "function".into(),
                function: crate::llm::FunctionDefinition {
                    name: fn_name,
                    description: format!(
                        "Transfer the conversation to the {} agent. {}",
                        handoff.name,
                        handoff.instructions.chars().take(100).collect::<String>()
                    ),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {},
                        "required": []
                    }),
                },
            });
        }

        defs
    }

    pub fn handoff_agent_names(&self) -> Vec<String> {
        self.handoffs.iter().map(|a| a.name.clone()).collect()
    }

    pub fn find_tool(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name)
    }

    pub fn find_handoff(&self, name: &str) -> Option<&Agent> {
        let normalized = name.to_lowercase().replace('_', " ");
        self.handoffs
            .iter()
            .find(|a| a.name.to_lowercase() == normalized)
    }
}

/// Builder pattern for constructing Agents ergonomically.
pub struct AgentBuilder {
    name: String,
    instructions: String,
    model: String,
    tools: Vec<Arc<dyn Tool>>,
    handoffs: Vec<Agent>,
    max_turns: usize,
}

impl AgentBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            instructions: String::new(),
            model: "gpt-4o".to_string(),
            tools: Vec::new(),
            handoffs: Vec::new(),
            max_turns: 10,
        }
    }

    pub fn instructions(mut self, instructions: &str) -> Self {
        self.instructions = instructions.to_string();
        self
    }

    pub fn model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    pub fn tool(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    pub fn tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.tools.extend(tools);
        self
    }

    pub fn handoff(mut self, agent: Agent) -> Self {
        self.handoffs.push(agent);
        self
    }

    pub fn handoffs(mut self, agents: Vec<Agent>) -> Self {
        self.handoffs.extend(agents);
        self
    }

    pub fn max_turns(mut self, max: usize) -> Self {
        self.max_turns = max;
        self
    }

    pub fn build(self) -> Agent {
        Agent {
            name: self.name,
            instructions: self.instructions,
            model: self.model,
            tools: self.tools,
            handoffs: self.handoffs,
            max_turns: self.max_turns,
        }
    }
}
