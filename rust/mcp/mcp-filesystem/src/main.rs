//! mcp-filesystem — MCP server for comprehensive filesystem operations.
//!
//! Tools:
//!   - read_file              Read the contents of a file
//!   - read_file_range        Read a portion of a file (offset + limit)
//!   - read_multiple_files    Read multiple files at once
//!   - write_file             Write content to a file (create or overwrite)
//!   - edit_file              Find/replace edits within a file
//!   - create_directory       Create a new directory (with parents)
//!   - list_directory         List files and directories in a path
//!   - directory_tree         Get a recursive directory tree structure
//!   - move_file              Move/rename a file or directory
//!   - copy_file              Copy a file or directory
//!   - delete_file            Delete a file or directory
//!   - get_file_info          Get file metadata
//!   - search_files           Search for files by glob pattern
//!   - file_exists            Check if a file/directory exists
//!   - get_allowed_directories List the allowed/accessible directories

use async_trait::async_trait;
use rust_mcp_schema::schema_utils::CallToolError;
use rust_mcp_schema::{
    CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, RpcError,
    Tool, ToolInputSchema,
};
use rust_mcp_sdk::mcp_server::ServerHandler;
use rust_mcp_sdk::schema::{
    Implementation, InitializeResult, ProtocolVersion, ServerCapabilities,
    ServerCapabilitiesTools,
};
use rust_mcp_sdk::{
    mcp_server::{server_runtime, McpServerOptions},
    McpServer, StdioTransport, ToMcpServerHandler, TransportOptions,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

mod fs_ops;
use fs_ops::*;

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

struct FilesystemHandler {
    allowed_dirs: Vec<PathBuf>,
}

impl FilesystemHandler {
    fn new(allowed_dirs: Vec<PathBuf>) -> Self {
        Self { allowed_dirs }
    }
}

#[async_trait]
impl ServerHandler for FilesystemHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "read_file".into(),
                    description: Some(
                        "Read the complete contents of a file. Returns the file content as a string. \
                        For large files, consider using read_file_range instead."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the file to read"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "read_file_range".into(),
                    description: Some(
                        "Read a portion of a file by specifying an offset and/or byte limit. \
                        Useful for reading large files in chunks."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the file"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "offset".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Byte offset to start reading from (0-based)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "limit".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Maximum number of bytes to read"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "read_multiple_files".into(),
                    description: Some(
                        "Read the contents of multiple files at once. Returns an array of results, \
                        each containing the path, content (on success), or error (on failure)."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["paths".to_string()],
                        Some(BTreeMap::from([(
                            "paths".to_string(),
                            serde_json::json!({
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Array of absolute file paths to read"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "write_file".into(),
                    description: Some(
                        "Write content to a file. Creates the file if it doesn't exist, \
                        or overwrites it if it does. Parent directories are created automatically."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string(), "content".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the file to write"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "content".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Content to write to the file"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "edit_file".into(),
                    description: Some(
                        "Perform a find-and-replace operation within a file. \
                        Replaces all occurrences of old_string with new_string. \
                        Returns the number of replacements made."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string(), "old_string".to_string(), "new_string".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the file to edit"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "old_string".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Text to search for (must exist in the file)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "new_string".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Text to replace with"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "create_directory".into(),
                    description: Some(
                        "Create a new directory. Works like `mkdir -p` — creates parent \
                        directories as needed. Succeeds silently if the directory already exists."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the directory to create"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "list_directory".into(),
                    description: Some(
                        "List files and directories in a path. Returns an array of entries \
                        with name, path, type, and size. Use recursive=true for deep listing."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "recursive".to_string(),
                                serde_json::json!({
                                    "type": "boolean",
                                    "description": "List recursively (default: false)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "directory_tree".into(),
                    description: Some(
                        "Get a recursive directory tree structure. Returns a nested tree \
                        of entries with children for subdirectories."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the root directory"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "move_file".into(),
                    description: Some(
                        "Move or rename a file or directory. Works like `mv`. \
                        Parent directories of the destination are created automatically."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["source".to_string(), "destination".to_string()],
                        Some(BTreeMap::from([
                            (
                                "source".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Source path (file or directory)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "destination".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Destination path"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "copy_file".into(),
                    description: Some(
                        "Copy a file or directory. For directories, recursive=true is required. \
                        Parent directories of the destination are created automatically."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["source".to_string(), "destination".to_string()],
                        Some(BTreeMap::from([
                            (
                                "source".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Source path (file or directory)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "destination".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Destination path"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "recursive".to_string(),
                                serde_json::json!({
                                    "type": "boolean",
                                    "description": "Copy directories recursively (default: false)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "delete_file".into(),
                    description: Some(
                        "Delete a file or directory. For directories, recursive=true is required. \
                        This operation is irreversible."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the file or directory to delete"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "recursive".to_string(),
                                serde_json::json!({
                                    "type": "boolean",
                                    "description": "Delete directories recursively (default: false)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "get_file_info".into(),
                    description: Some(
                        "Get detailed metadata about a file or directory including size, \
                        permissions, modification time, creation time, and access time."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the file or directory"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "search_files".into(),
                    description: Some(
                        "Search for files by glob pattern within a directory. \
                        Supports standard glob patterns (e.g., **/*.rs, *.toml, data/**/*.csv)."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["pattern".to_string()],
                        Some(BTreeMap::from([
                            (
                                "pattern".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Glob pattern to match (e.g., '**/*.rs', '*.toml')"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "root_path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Root directory to search in (default: first allowed directory)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "file_exists".into(),
                    description: Some(
                        "Check if a file or directory exists at the given path. \
                        Returns a boolean."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to check"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "get_allowed_directories".into(),
                    description: Some(
                        "List the directories that this server is allowed to access. \
                        All filesystem operations are restricted to these directories."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        None,
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
            ],
            meta: None,
            next_cursor: None,
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<CallToolResult, CallToolError> {
        let args = params.arguments.unwrap_or_default();

        match params.name.as_str() {
            "read_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match read_file(&self.allowed_dirs, path) {
                    Ok(content) => {
                        let text = serde_json::json!({
                            "path": path,
                            "content": content
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "read_file_range" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let offset = args.get("offset").and_then(|v| v.as_u64());
                let limit = args.get("limit").and_then(|v| v.as_u64());

                match read_file_range(&self.allowed_dirs, path, offset, limit) {
                    Ok(content) => {
                        let text = serde_json::json!({
                            "path": path,
                            "content": content,
                            "offset": offset,
                            "limit": limit
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "read_multiple_files" => {
                let paths: Vec<String> = args
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Missing required argument: paths",
                        ))
                    })?;

                match read_multiple_files(&self.allowed_dirs, &paths) {
                    Ok(results) => {
                        let text = serde_json::to_string_pretty(&results)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "write_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let content = args.get("content").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: content",
                    ))
                })?;

                match write_file(&self.allowed_dirs, path, content) {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Successfully wrote {} bytes to {}", content.len(), path)
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "edit_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let old_string = args.get("old_string").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: old_string",
                    ))
                })?;
                let new_string = args.get("new_string").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: new_string",
                    ))
                })?;

                match edit_file(&self.allowed_dirs, path, old_string, new_string) {
                    Ok(result) => {
                        let text = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "create_directory" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match create_directory(&self.allowed_dirs, path) {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Directory created: {}", path)
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "list_directory" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);

                match list_directory(&self.allowed_dirs, path, recursive) {
                    Ok(entries) => {
                        let text = serde_json::to_string_pretty(&entries)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "directory_tree" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match directory_tree(&self.allowed_dirs, path) {
                    Ok(tree) => {
                        let text = serde_json::to_string_pretty(&tree)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "move_file" => {
                let source = args.get("source").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: source",
                    ))
                })?;
                let destination = args.get("destination").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: destination",
                    ))
                })?;

                match move_file(&self.allowed_dirs, source, destination) {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Moved '{}' to '{}'", source, destination)
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "copy_file" => {
                let source = args.get("source").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: source",
                    ))
                })?;
                let destination = args.get("destination").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: destination",
                    ))
                })?;
                let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);

                match copy_file(&self.allowed_dirs, source, destination, recursive) {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Copied '{}' to '{}'", source, destination)
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "delete_file" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let recursive = args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);

                match delete_file(&self.allowed_dirs, path, recursive) {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Deleted: {}", path)
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "get_file_info" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match get_file_info(&self.allowed_dirs, path) {
                    Ok(info) => {
                        let text = serde_json::to_string_pretty(&info)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "search_files" => {
                let pattern = args.get("pattern").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: pattern",
                    ))
                })?;
                let root_path = args.get("root_path").and_then(|v| v.as_str());

                match search_files(&self.allowed_dirs, pattern, root_path) {
                    Ok(results) => {
                        let text = serde_json::to_string_pretty(&results)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "file_exists" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match file_exists(&self.allowed_dirs, path) {
                    Ok(exists) => {
                        let text = serde_json::json!({
                            "path": path,
                            "exists": exists
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "get_allowed_directories" => {
                let dirs = get_allowed_directories(&self.allowed_dirs);
                let text = serde_json::json!({
                    "allowed_directories": dirs
                })
                .to_string();
                Ok(CallToolResult::text_content(vec![
                    rust_mcp_schema::TextContent::new(text, None, None),
                ]))
            }
            _ => Err(CallToolError::unknown_tool(params.name)),
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    // Collect allowed directories from CLI arguments.
    // If none are provided, default to the current working directory.
    let allowed_dirs: Vec<PathBuf> = {
        let dirs: Vec<PathBuf> = std::env::args()
            .skip(1) // skip binary name
            .map(PathBuf::from)
            .collect();

        if dirs.is_empty() {
            vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
        } else {
            dirs
        }
    };

    let server_details = InitializeResult {
        server_info: Implementation {
            name: "mcp-filesystem".into(),
            version: "1.0.0".into(),
            description: Some("MCP server for comprehensive filesystem operations".into()),
            icons: vec![],
            title: Some("Filesystem MCP Server".into()),
            website_url: None,
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        protocol_version: ProtocolVersion::V2025_11_25.into(),
        instructions: None,
        meta: None,
    };

    let transport = StdioTransport::new(TransportOptions::default())?;
    let handler = FilesystemHandler::new(allowed_dirs).to_mcp_server_handler();
    let server = server_runtime::create_server(McpServerOptions {
        transport,
        handler,
        server_details,
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    server.start().await?;
    Ok(())
}
