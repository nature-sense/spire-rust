// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use rust_mcp_sdk::schema::*;
use std::collections::BTreeMap;

/// Represents a registered tool definition.
#[allow(dead_code)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: ToolInputSchema,
}

/// Returns the list of all available tools.
pub fn get_tools() -> Vec<Tool> {
    vec![
        explain_code_tool(),
        search_codebase_tool(),
        analyze_dependencies_tool(),
        get_code_metrics_tool(),
    ]
}

/// Creates a ToolInputSchema with the given properties and required fields.
fn make_input_schema(
    properties: Vec<(&str, serde_json::Value)>,
    required: Vec<&str>,
) -> ToolInputSchema {
    let mut props = BTreeMap::new();
    for (name, schema_value) in properties {
        // schema_value should be a JSON object like {"type": "string", "description": "..."}
        if let serde_json::Value::Object(map) = schema_value {
            props.insert(name.to_string(), map);
        }
    }
    ToolInputSchema::new(
        required.into_iter().map(|s| s.to_string()).collect(),
        if props.is_empty() {
            None
        } else {
            Some(props)
        },
        None, // $schema
    )
}

/// Tool: explain_code
/// Explains a given code snippet or file path.
fn explain_code_tool() -> Tool {
    Tool {
        name: "explain_code".into(),
        description: Some(
            "Explains a code snippet or file, providing context and analysis".into(),
        ),
        input_schema: make_input_schema(
            vec![
                (
                    "code",
                    serde_json::json!({
                        "type": "string",
                        "description": "The code snippet or file path to explain"
                    }),
                ),
                (
                    "language",
                    serde_json::json!({
                        "type": "string",
                        "description": "The programming language (optional, auto-detected if omitted)"
                    }),
                ),
            ],
            vec!["code"],
        ),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
        title: Some("Explain Code".into()),
    }
}

/// Tool: search_codebase
/// Searches the codebase using regex or semantic search.
fn search_codebase_tool() -> Tool {
    Tool {
        name: "search_codebase".into(),
        description: Some(
            "Searches the codebase using regex patterns or semantic search queries".into(),
        ),
        input_schema: make_input_schema(
            vec![
                (
                    "query",
                    serde_json::json!({
                        "type": "string",
                        "description": "The search query (regex or natural language)"
                    }),
                ),
                (
                    "mode",
                    serde_json::json!({
                        "type": "string",
                        "description": "Search mode: 'regex' or 'semantic'",
                        "enum": ["regex", "semantic"]
                    }),
                ),
                (
                    "path",
                    serde_json::json!({
                        "type": "string",
                        "description": "Optional path to scope the search"
                    }),
                ),
            ],
            vec!["query"],
        ),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
        title: Some("Search Codebase".into()),
    }
}

/// Tool: analyze_dependencies
/// Analyzes dependencies of a given module or file.
fn analyze_dependencies_tool() -> Tool {
    Tool {
        name: "analyze_dependencies".into(),
        description: Some(
            "Analyzes the dependency graph for a given file or module".into(),
        ),
        input_schema: make_input_schema(
            vec![
                (
                    "path",
                    serde_json::json!({
                        "type": "string",
                        "description": "The file or module path to analyze"
                    }),
                ),
                (
                    "depth",
                    serde_json::json!({
                        "type": "integer",
                        "description": "Maximum depth for dependency traversal (default: 1)"
                    }),
                ),
            ],
            vec!["path"],
        ),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
        title: Some("Analyze Dependencies".into()),
    }
}

/// Tool: get_code_metrics
/// Retrieves code quality metrics for a file or project.
fn get_code_metrics_tool() -> Tool {
    Tool {
        name: "get_code_metrics".into(),
        description: Some(
            "Retrieves code quality metrics such as complexity, line count, and more".into(),
        ),
        input_schema: make_input_schema(
            vec![
                (
                    "path",
                    serde_json::json!({
                        "type": "string",
                        "description": "The file or directory path to analyze"
                    }),
                ),
                (
                    "metrics",
                    serde_json::json!({
                        "type": "array",
                        "description": "Specific metrics to compute (e.g., 'complexity', 'loc', 'coverage')",
                        "items": {"type": "string"}
                    }),
                ),
            ],
            vec!["path"],
        ),
        annotations: None,
        execution: None,
        icons: vec![],
        meta: None,
        output_schema: None,
        title: Some("Get Code Metrics".into()),
    }
}

/// Handles a tool call by dispatching to the appropriate implementation.
pub fn handle_tool_call(
    name: &str,
    arguments: serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<CallToolResult, rust_mcp_sdk::schema::schema_utils::CallToolError> {
    match name {
        "explain_code" => handle_explain_code(arguments),
        "search_codebase" => handle_search_codebase(arguments),
        "analyze_dependencies" => handle_analyze_dependencies(arguments),
        "get_code_metrics" => handle_get_code_metrics(arguments),
        _ => Err(rust_mcp_sdk::schema::schema_utils::CallToolError::unknown_tool(name.to_string())),
    }
}

fn handle_explain_code(
    arguments: serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<CallToolResult, rust_mcp_sdk::schema::schema_utils::CallToolError> {
    let code = arguments
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // TODO: Implement actual code explanation logic
    let result = format!(
        "Code explanation for: {}\n\nThis is a placeholder. Full implementation coming soon.",
        code
    );

    Ok(CallToolResult::text_content(vec![TextContent::new(
        result,
        None,
        None,
    )]))
}

fn handle_search_codebase(
    arguments: serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<CallToolResult, rust_mcp_sdk::schema::schema_utils::CallToolError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mode = arguments
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("regex");

    // TODO: Implement actual search logic
    let result = format!(
        "Search results for '{}' (mode: {})\n\nThis is a placeholder. Full implementation coming soon.",
        query, mode
    );

    Ok(CallToolResult::text_content(vec![TextContent::new(
        result,
        None,
        None,
    )]))
}

fn handle_analyze_dependencies(
    arguments: serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<CallToolResult, rust_mcp_sdk::schema::schema_utils::CallToolError> {
    let path = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let depth = arguments
        .get("depth")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);

    // TODO: Implement actual dependency analysis
    let result = format!(
        "Dependency analysis for '{}' (depth: {})\n\nThis is a placeholder. Full implementation coming soon.",
        path, depth
    );

    Ok(CallToolResult::text_content(vec![TextContent::new(
        result,
        None,
        None,
    )]))
}

fn handle_get_code_metrics(
    arguments: serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<CallToolResult, rust_mcp_sdk::schema::schema_utils::CallToolError> {
    let path = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // TODO: Implement actual metrics computation
    let result = format!(
        "Code metrics for '{}'\n\nThis is a placeholder. Full implementation coming soon.",
        path
    );

    Ok(CallToolResult::text_content(vec![TextContent::new(
        result,
        None,
        None,
    )]))
}
