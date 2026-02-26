use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

use super::trait_def::Tool;
use crate::safety::guardrails::{is_dangerous, DryRunMode, requires_confirmation};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const MAX_OUTPUT_CHARS: usize = 30_000;

fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_CHARS {
        return s.to_string();
    }
    let head_size = MAX_OUTPUT_CHARS * 2 / 3;
    let tail_size = MAX_OUTPUT_CHARS / 3;
    let head = &s[..head_size];
    let tail = &s[s.len() - tail_size..];
    let total_lines = s.lines().count();
    let head_lines = head.lines().count();
    let tail_lines = tail.lines().count();
    let omitted = total_lines.saturating_sub(head_lines + tail_lines);
    format!(
        "{}\n\n[... TRUNCATED: {} total lines, {} lines omitted from middle ...]\n\n{}",
        head, total_lines, omitted, tail
    )
}

#[derive(Debug, Clone)]
pub struct ShellTool {
    pub allow_sudo: bool,
    pub working_dir: PathBuf,
    pub default_timeout: u64,
}

impl ShellTool {
    pub fn new(working_dir: PathBuf) -> Self {
        Self {
            allow_sudo: false,
            working_dir,
            default_timeout: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn with_sudo(mut self) -> Self {
        self.allow_sudo = true;
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.default_timeout = secs;
        self
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout, stderr, and exit code. \
         Set use_sudo to true for privileged commands. \
         Set timeout_secs to override the default timeout (120s) for long-running commands like find, grep -r, etc."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "use_sudo": {
                    "type": "boolean",
                    "description": "Whether to prefix with sudo",
                    "default": false
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds for this command (default 120, max 600). Use higher values for filesystem-wide searches, large builds, or network operations.",
                    "default": 120
                },
                "interactive": {
                    "type": "boolean",
                    "description": "Set to true for commands that require user input (e.g. npx create-*, installers with prompts, y/n confirmations). The command runs with full terminal access so the user can see prompts and type responses. Output is NOT captured — only the exit code is returned.",
                    "default": false
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: command"))?;
        let use_sudo = args["use_sudo"].as_bool().unwrap_or(false);
        let interactive = args["interactive"].as_bool().unwrap_or(false);
        let timeout_secs = args["timeout_secs"]
            .as_u64()
            .unwrap_or(self.default_timeout)
            .min(MAX_TIMEOUT_SECS);

        let full_cmd = if use_sudo && self.allow_sudo {
            format!("sudo {}", command)
        } else if use_sudo && !self.allow_sudo {
            return Ok(json!({
                "error": "sudo is not enabled. Restart the session with --sudo to enable privileged commands.",
                "command": command
            }));
        } else {
            command.to_string()
        };

        if is_dangerous(&full_cmd) {
            warn!("Blocked dangerous command: {}", full_cmd);
            return Ok(json!({
                "error": "Command blocked by safety guardrails",
                "command": full_cmd,
                "reason": "Matches a known dangerous pattern"
            }));
        }

        if DryRunMode::is_active() {
            info!("[DRY RUN] Would execute: {}", full_cmd);
            return Ok(json!({
                "dry_run": true,
                "command": full_cmd,
                "message": "Command was not executed (dry-run mode)"
            }));
        }

        if requires_confirmation(&full_cmd) {
            info!("Command requires confirmation: {}", full_cmd);
        }

        if interactive {
            return self.execute_interactive(&full_cmd, timeout_secs).await;
        }

        info!("Executing (timeout {}s): {}", timeout_secs, full_cmd);

        let output = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&full_cmd)
                .current_dir(&self.working_dir)
                .output(),
        )
        .await;

        match output {
            Ok(Ok(output)) => {
                let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let raw_stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                let stdout = truncate_output(&raw_stdout);
                let stderr = truncate_output(&raw_stderr);

                let mut result = json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": exit_code,
                    "command": full_cmd
                });

                if raw_stdout.len() > MAX_OUTPUT_CHARS {
                    result["stdout_truncated"] = json!(true);
                    result["stdout_original_bytes"] = json!(raw_stdout.len());
                    result["stdout_total_lines"] = json!(raw_stdout.lines().count());
                }

                Ok(result)
            }
            Ok(Err(e)) => Ok(json!({
                "error": format!("Failed to execute command: {}", e),
                "command": full_cmd
            })),
            Err(_) => Ok(json!({
                "error": format!("Command timed out after {}s. Try: (1) increasing timeout_secs, (2) narrowing the search scope, or (3) using a faster alternative like mdfind/locate.", timeout_secs),
                "command": full_cmd,
                "timed_out": true,
                "timeout_secs": timeout_secs
            })),
        }
    }
}

impl ShellTool {
    async fn execute_interactive(&self, full_cmd: &str, timeout_secs: u64) -> anyhow::Result<Value> {
        use std::process::Stdio;

        info!("Executing INTERACTIVE (timeout {}s): {}", timeout_secs, full_cmd);

        eprintln!(
            "\n  \x1b[48;5;208m\x1b[30m\x1b[1m INTERACTIVE \x1b[0m \x1b[2mUser input required — respond to prompts below\x1b[0m"
        );
        eprintln!("  \x1b[90m{}\x1b[0m", "─".repeat(58));

        let result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(full_cmd)
                .current_dir(&self.working_dir)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status(),
        )
        .await;

        eprintln!("  \x1b[90m{}\x1b[0m", "─".repeat(58));

        match result {
            Ok(Ok(status)) => {
                let exit_code = status.code().unwrap_or(-1);
                Ok(json!({
                    "exit_code": exit_code,
                    "command": full_cmd,
                    "interactive": true,
                    "note": "Command ran interactively with full terminal access. Output was displayed directly to the user."
                }))
            }
            Ok(Err(e)) => Ok(json!({
                "error": format!("Failed to execute command: {}", e),
                "command": full_cmd
            })),
            Err(_) => Ok(json!({
                "error": format!("Interactive command timed out after {}s.", timeout_secs),
                "command": full_cmd,
                "timed_out": true,
                "timeout_secs": timeout_secs
            })),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunScriptTool {
    pub working_dir: PathBuf,
}

#[async_trait]
impl Tool for RunScriptTool {
    fn name(&self) -> &str {
        "run_script"
    }

    fn description(&self) -> &str {
        "Write a script to a temporary file and execute it. \
         Supports bash, python, node, and other interpreters."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The script content"
                },
                "language": {
                    "type": "string",
                    "description": "Script language: bash, python, node, ruby",
                    "enum": ["bash", "python", "node", "ruby"]
                }
            },
            "required": ["content", "language"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: content"))?;
        let language = args["language"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: language"))?;

        let (extension, interpreter) = match language {
            "bash" => ("sh", "bash"),
            "python" => ("py", "python3"),
            "node" => ("js", "node"),
            "ruby" => ("rb", "ruby"),
            other => return Ok(json!({"error": format!("Unsupported language: {}", other)})),
        };

        let tmp_path = std::env::temp_dir().join(format!(
            "agterm_script_{}.{}",
            uuid::Uuid::new_v4(),
            extension
        ));

        tokio::fs::write(&tmp_path, content).await?;

        if language == "bash" {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o755);
                tokio::fs::set_permissions(&tmp_path, perms).await?;
            }
        }

        let output = tokio::process::Command::new(interpreter)
            .arg(&tmp_path)
            .current_dir(&self.working_dir)
            .output()
            .await?;

        let _ = tokio::fs::remove_file(&tmp_path).await;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "exit_code": output.status.code().unwrap_or(-1),
            "language": language
        }))
    }
}
