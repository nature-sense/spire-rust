//! Cargo build execution — run cargo commands and return structured results.
//!
//! This module is extracted from `tools/cargo-build-tool/` and adapted to work
//! as a module within the mcp-cargo MCP server.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;
use tokio::process::Command;
use tracing::{info, error};

/// Structured output from a cargo build/command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutput {
    pub success: bool,
    pub command: String,
    pub duration_secs: f64,
    pub output: String,
    pub errors: Vec<ErrorInfo>,
    pub warnings: Vec<WarningInfo>,
    pub artifacts: Vec<ArtifactInfo>,
}

/// A parsed error from build output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// A parsed warning from build output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarningInfo {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// Information about a build artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub path: String,
    pub kind: String,
    pub profile: String,
}

/// Run `cargo build` with the given options.
///
/// # Arguments
/// * `project_root` - Path to the Cargo project directory
/// * `mode` - "debug" or "release"
/// * `scope` - "package" or "workspace"
/// * `package` - Optional package name for `cargo build -p <package>`
/// * `target` - Optional target triple for cross-compilation
/// * `features` - Optional list of feature flags
/// * `jobs` - Number of parallel jobs (0 = auto)
pub async fn run_cargo_build(
    project_root: &str,
    mode: &str,
    scope: &str,
    package: Option<&str>,
    target: Option<&str>,
    features: &[String],
    jobs: usize,
) -> Result<BuildOutput, String> {
    let root = Path::new(project_root);
    if !root.join("Cargo.toml").exists() {
        return Err(format!("No Cargo.toml found in {}", project_root));
    }

    let mut args = vec!["build".to_string()];

    // Mode
    if mode == "release" {
        args.push("--release".to_string());
    }

    // Scope
    if scope == "workspace" {
        args.push("--workspace".to_string());
    }

    // Package (single crate build)
    if let Some(pkg) = package {
        args.push("-p".to_string());
        args.push(pkg.to_string());
    }

    // Target triple
    if let Some(triple) = target {
        args.push("--target".to_string());
        args.push(triple.to_string());
    }

    // Feature flags
    for feature in features {
        args.push("--features".to_string());
        args.push(feature.clone());
    }

    // Parallel jobs
    if jobs > 0 {
        args.push("-j".to_string());
        args.push(jobs.to_string());
    }

    // Use JSON message format for structured output
    args.push("--message-format".to_string());
    args.push("json".to_string());

    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_cargo(project_root, &args_refs).await
}


/// Run a generic cargo command (check, test, clippy, fmt, etc.).
pub async fn run_cargo_command(project_root: &str, subcommand: &[&str]) -> Result<BuildOutput, String> {
    let root = Path::new(project_root);
    if !root.join("Cargo.toml").exists() {
        return Err(format!("No Cargo.toml found in {}", project_root));
    }

    let mut args = Vec::from(subcommand);

    // For test, add JSON output
    if subcommand.first().copied() == Some("test") {
        args.push("--message-format");
        args.push("json");
    }

    run_cargo(project_root, &args).await
}

/// Execute a cargo command and parse the output.
async fn run_cargo(project_root: &str, args: &[&str]) -> Result<BuildOutput, String> {
    let start = Instant::now();

    let command_str = format!("cargo {}", args.join(" "));
    info!("executing: {} in {}", command_str, project_root);

    let mut cmd = Command::new("cargo");
    cmd.current_dir(project_root);
    cmd.args(args);

    // Set environment for colored output
    cmd.env("CARGO_TERM_COLOR", "never");

    let output = cmd.output().await.map_err(|e| {
        error!("failed to execute cargo: {}", e);
        format!("Failed to execute cargo: {e}")
    })?;

    let duration = start.elapsed().as_secs_f64();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    let success = output.status.success();

    // Parse JSON messages from stdout (cargo --message-format=json)
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut artifacts = Vec::new();
    let mut rendered_messages = Vec::new();

    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
            match msg.get("reason").and_then(|v| v.as_str()) {
                Some("compiler-message") => {
                    if let Some(message) = msg.get("message") {
                        let msg_text = message
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let level = message
                            .get("level")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let file = message
                            .get("spans")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|span| span.get("file"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let line_num = message
                            .get("spans")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|span| span.get("line_start"))
                            .and_then(|v| v.as_u64())
                            .map(|n| n as u32);

                        let col = message
                            .get("spans")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|span| span.get("column_start"))
                            .and_then(|v| v.as_u64())
                            .map(|n| n as u32);

                        match level {
                            "error" | "failure" => {
                                errors.push(ErrorInfo {
                                    message: msg_text,
                                    file,
                                    line: line_num,
                                    column: col,
                                });
                            }
                            "warning" => {
                                warnings.push(WarningInfo {
                                    message: msg_text,
                                    file,
                                    line: line_num,
                                    column: col,
                                });
                            }
                            _ => {}
                        }

                        if let Some(rendered) = message.get("rendered").and_then(|v| v.as_str()) {
                            rendered_messages.push(rendered.to_string());
                        }
                    }
                }
                Some("compiler-artifact") => {
                    let path = msg
                        .get("filenames")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let kind = msg
                        .get("target")
                        .and_then(|t| t.get("kind"))
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let profile = msg
                        .get("profile")
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    if let Some(p) = path {
                        artifacts.push(ArtifactInfo {
                            path: p,
                            kind,
                            profile,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    let combined = if stderr.is_empty() {
        stdout.clone()
    } else {
        format!("{}\n{}", stdout, stderr)
    };

    let is_json = stdout.lines().any(|l| l.starts_with('{'));
    let output_text = if is_json {
        let mut text = stderr.clone();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&rendered_messages.join("\n"));
        text
    } else {
        combined.clone()
    };

    // Fallback: if no JSON messages were parsed, try regex-based parsing
    if errors.is_empty() && warnings.is_empty() && !success {
        parse_errors_from_text(&combined, &mut errors, &mut warnings);
    }

    let command_str = format!("cargo {}", args.join(" "));

    Ok(BuildOutput {
        success,
        command: command_str,
        duration_secs: duration,
        output: output_text,
        errors,
        warnings,
        artifacts,
    })
}

/// Fallback regex-based error/warning parsing for non-JSON output.
fn parse_errors_from_text(text: &str, errors: &mut Vec<ErrorInfo>, warnings: &mut Vec<WarningInfo>) {
    // Pattern: file:line:col: level: message
    let re = regex::Regex::new(
        r"^(.+?):(\d+):(\d+):\s+(error|warning|failure)\b\s*\[?(.*?)\]?\s*$",
    )
    .unwrap();

    for line in text.lines() {
        if let Some(caps) = re.captures(line) {
            let file = caps.get(1).map(|m| m.as_str().to_string());
            let line_num = caps
                .get(2)
                .and_then(|m| m.as_str().parse::<u32>().ok());
            let col = caps
                .get(3)
                .and_then(|m| m.as_str().parse::<u32>().ok());
            let level = caps.get(4).map(|m| m.as_str()).unwrap_or("");
            let message = caps.get(5).map(|m| m.as_str().to_string()).unwrap_or_default();

            match level {
                "error" | "failure" => {
                    errors.push(ErrorInfo {
                        message,
                        file,
                        line: line_num,
                        column: col,
                    });
                }
                "warning" => {
                    warnings.push(WarningInfo {
                        message,
                        file,
                        line: line_num,
                        column: col,
                    });
                }
                _ => {}
            }
        }
    }
}
