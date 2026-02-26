use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::info;

use super::trait_def::Tool;

#[derive(Debug, Clone)]
pub struct CheckProcessTool;

#[async_trait]
impl Tool for CheckProcessTool {
    fn name(&self) -> &str {
        "check_process"
    }

    fn description(&self) -> &str {
        "Check if a process is running by name. Returns matching process info."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Process name to search for"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;

        info!("Checking process: {}", name);

        let output = tokio::process::Command::new("ps")
            .args(["aux"])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let matching: Vec<&str> = stdout
            .lines()
            .filter(|line| line.contains(name) && !line.contains("grep"))
            .collect();

        Ok(json!({
            "process_name": name,
            "running": !matching.is_empty(),
            "count": matching.len(),
            "matches": matching
        }))
    }
}

#[derive(Debug, Clone)]
pub struct KillProcessTool;

#[async_trait]
impl Tool for KillProcessTool {
    fn name(&self) -> &str {
        "kill_process"
    }

    fn description(&self) -> &str {
        "Kill a process by PID or name. Use signal to specify the signal (default: SIGTERM)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Process PID (number) or name to kill"
                },
                "signal": {
                    "type": "string",
                    "description": "Signal to send: SIGTERM, SIGKILL, SIGHUP, etc.",
                    "default": "SIGTERM"
                }
            },
            "required": ["target"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let target = args["target"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: target"))?;
        let signal = args["signal"].as_str().unwrap_or("SIGTERM");

        info!("Killing process: {} with signal {}", target, signal);

        let is_pid = target.parse::<u32>().is_ok();

        let output = if is_pid {
            tokio::process::Command::new("kill")
                .args([&format!("-s {}", signal), target])
                .output()
                .await?
        } else {
            tokio::process::Command::new("pkill")
                .args([&format!("-{}", signal.replace("SIG", "")), target])
                .output()
                .await?
        };

        Ok(json!({
            "target": target,
            "signal": signal,
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr)
        }))
    }
}
