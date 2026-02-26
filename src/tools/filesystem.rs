use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use tracing::info;

use super::trait_def::Tool;

const MAX_FILE_CONTENT_CHARS: usize = 30_000;
const MAX_DIR_ENTRIES: usize = 200;

#[derive(Debug, Clone)]
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

        info!("Reading file: {}", path);

        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let metadata = tokio::fs::metadata(path).await?;
                let original_len = content.len();
                let total_lines = content.lines().count();

                let (display_content, truncated) = if content.len() > MAX_FILE_CONTENT_CHARS {
                    let head = &content[..MAX_FILE_CONTENT_CHARS * 2 / 3];
                    let tail = &content[content.len() - MAX_FILE_CONTENT_CHARS / 3..];
                    (
                        format!(
                            "{}\n\n[... TRUNCATED: {} total lines, showing head + tail ...]\n\n{}",
                            head, total_lines, tail
                        ),
                        true,
                    )
                } else {
                    (content, false)
                };

                let mut result = json!({
                    "content": display_content,
                    "path": path,
                    "size_bytes": metadata.len(),
                    "total_lines": total_lines,
                });

                if truncated {
                    result["truncated"] = json!(true);
                    result["original_chars"] = json!(original_len);
                }

                Ok(result)
            }
            Err(e) => Ok(json!({
                "error": format!("Failed to read file: {}", e),
                "path": path
            })),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WriteFileTool {
    pub interactive: bool,
}

impl WriteFileTool {
    pub fn new(interactive: bool) -> Self {
        Self { interactive }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. \
         In interactive mode, shows a diff preview and asks for approval before writing."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;
        let new_content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: content"))?;

        info!("Writing file: {}", path);

        let file_path = PathBuf::from(path);
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let old_content = tokio::fs::read_to_string(path).await.unwrap_or_default();
        let is_new = old_content.is_empty()
            && tokio::fs::metadata(path).await.is_err();

        if self.interactive {
            use crate::display::diff;

            if is_new {
                diff::render_new_file(path, new_content);
            } else {
                let diff_result = diff::compute_diff(&old_content, new_content);
                if diff_result.additions == 0 && diff_result.deletions == 0 {
                    return Ok(json!({
                        "success": true,
                        "path": path,
                        "message": "File content unchanged, no write needed"
                    }));
                }
                diff::render_diff(path, &diff_result);
            }

            let decision = diff::prompt_review(path);

            match decision {
                diff::ReviewDecision::AcceptAll => {
                    match tokio::fs::write(path, new_content).await {
                        Ok(()) => {
                            diff::render_review_summary(
                                if is_new {
                                    new_content.lines().count()
                                } else {
                                    let d = diff::compute_diff(&old_content, new_content);
                                    d.additions + d.deletions
                                },
                                0,
                            );
                            Ok(json!({
                                "success": true,
                                "path": path,
                                "bytes_written": new_content.len(),
                                "decision": "accepted"
                            }))
                        }
                        Err(e) => Ok(json!({
                            "error": format!("Failed to write file: {}", e),
                            "path": path
                        })),
                    }
                }

                diff::ReviewDecision::RejectAll => {
                    diff::render_review_summary(0, 1);
                    Ok(json!({
                        "success": false,
                        "path": path,
                        "message": "User rejected all changes",
                        "decision": "rejected"
                    }))
                }

                diff::ReviewDecision::PerLine => {
                    let diff_result = diff::compute_diff(&old_content, new_content);
                    let decisions = diff::review_per_line(&diff_result);

                    let accepted = decisions.iter().filter(|d| **d).count();
                    let rejected = decisions.len() - accepted;

                    if accepted == 0 {
                        diff::render_review_summary(0, rejected);
                        return Ok(json!({
                            "success": false,
                            "path": path,
                            "message": "User rejected all changes",
                            "decision": "rejected"
                        }));
                    }

                    let final_content = diff::apply_decisions(
                        &old_content,
                        &diff_result,
                        &decisions,
                    );

                    match tokio::fs::write(path, &final_content).await {
                        Ok(()) => {
                            diff::render_review_summary(accepted, rejected);
                            Ok(json!({
                                "success": true,
                                "path": path,
                                "bytes_written": final_content.len(),
                                "decision": "partial",
                                "accepted": accepted,
                                "rejected": rejected
                            }))
                        }
                        Err(e) => Ok(json!({
                            "error": format!("Failed to write file: {}", e),
                            "path": path
                        })),
                    }
                }
            }
        } else {
            // Non-interactive: write directly
            match tokio::fs::write(path, new_content).await {
                Ok(()) => Ok(json!({
                    "success": true,
                    "path": path,
                    "bytes_written": new_content.len()
                })),
                Err(e) => Ok(json!({
                    "error": format!("Failed to write file: {}", e),
                    "path": path
                })),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListDirectoryTool;

#[async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List contents of a directory with metadata (name, size, type, permissions)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: path"))?;

        info!("Listing directory: {}", path);

        let mut entries = Vec::new();
        let mut dir = match tokio::fs::read_dir(path).await {
            Ok(d) => d,
            Err(e) => {
                return Ok(json!({
                    "error": format!("Failed to read directory: {}", e),
                    "path": path
                }))
            }
        };

        let mut total_count = 0usize;
        while let Some(entry) = dir.next_entry().await? {
            total_count += 1;
            if entries.len() < MAX_DIR_ENTRIES {
                let metadata = entry.metadata().await?;
                let file_type = if metadata.is_dir() {
                    "directory"
                } else if metadata.is_symlink() {
                    "symlink"
                } else {
                    "file"
                };

                entries.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "type": file_type,
                    "size_bytes": metadata.len(),
                    "readonly": metadata.permissions().readonly(),
                }));
            }
        }

        let mut result = json!({
            "path": path,
            "count": total_count,
            "entries": entries
        });

        if total_count > MAX_DIR_ENTRIES {
            result["truncated"] = json!(true);
            result["showing"] = json!(MAX_DIR_ENTRIES);
            result["total"] = json!(total_count);
        }

        Ok(result)
    }
}
