//! Node.js build execution — run npm/pnpm/yarn commands and return structured results.
//!
//! This module is extracted from `tools/node-build-tool/` and adapted to work
//! as a module within the mcp-node MCP server.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;
use tokio::process::Command;
use tracing::{info, error};

/// Structured output from a node command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutput {
    pub success: bool,
    pub command: String,
    pub package_manager: String,
    pub duration_secs: f64,
    pub output: String,
    pub errors: Vec<ErrorInfo>,
    pub warnings: Vec<WarningInfo>,
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

/// Detect the package manager for a project.
fn detect_package_manager(project_root: &str) -> String {
    let root = Path::new(project_root);
    if root.join("pnpm-lock.yaml").exists() || root.join("pnpm-workspace.yaml").exists() {
        "pnpm".to_string()
    } else if root.join("yarn.lock").exists() {
        "yarn".to_string()
    } else {
        "npm".to_string()
    }
}

/// Run an npm script (e.g. "build", "test").
pub async fn run_node_script(project_root: &str, script: &str, package_manager: &str) -> Result<BuildOutput, String> {
    let root = Path::new(project_root);
    if !root.join("package.json").exists() {
        return Err(format!("No package.json found in {}", project_root));
    }

    let pm = if package_manager == "auto" {
        detect_package_manager(project_root)
    } else {
        package_manager.to_string()
    };

    let args: Vec<&str> = match pm.as_str() {
        "pnpm" => vec!["run", script],
        "yarn" => vec!["run", script],
        _ => vec!["run", script],
    };

    run_command(project_root, &pm, &args).await
}

/// Run a generic node command (install, add, etc.).
pub async fn run_node_command(project_root: &str, extra_args: &[&str], package_manager: &str) -> Result<BuildOutput, String> {
    let root = Path::new(project_root);
    if !root.join("package.json").exists() {
        return Err(format!("No package.json found in {}", project_root));
    }

    let pm = if package_manager == "auto" {
        detect_package_manager(project_root)
    } else {
        package_manager.to_string()
    };

    let subcommand = match pm.as_str() {
        "pnpm" => "add",
        "yarn" => "add",
        _ => "install",
    };

    let mut args = vec![subcommand];
    args.extend_from_slice(extra_args);

    run_command(project_root, &pm, &args).await
}

/// Execute a package manager command and parse the output.
async fn run_command(project_root: &str, pm: &str, args: &[&str]) -> Result<BuildOutput, String> {
    let start = Instant::now();

    let command_str = format!("{} {}", pm, args.join(" "));
    info!("executing: {} in {}", command_str, project_root);

    let mut cmd = Command::new(pm);
    cmd.current_dir(project_root);
    cmd.args(args);

    let output = cmd.output().await.map_err(|e| {
        error!("failed to execute {}: {}", pm, e);
        format!("Failed to execute {}: {e}", pm)
    })?;

    let duration = start.elapsed().as_secs_f64();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = if stderr.is_empty() {
        stdout.clone()
    } else {
        format!("{}\n{}", stdout, stderr)
    };

    let success = output.status.success();

    // Parse errors and warnings from output
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Common error/warning patterns for npm/pnpm/yarn
    // Match only lines that START with an error/warning marker (not words containing "error")
    let error_re = regex::Regex::new(
        r"(?i)^\s*(error|ERR!|ERR_OUT|FAILED)\s*:?\s*(.*?)$",
    )
    .unwrap();
    let warning_re = regex::Regex::new(
        r"(?i)^\s*(warning|WARN)\s*:?\s*(.*?)$",
    )
    .unwrap();

    for line in combined.lines() {
        if let Some(caps) = error_re.captures(line) {
            let msg = caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
            if !msg.is_empty() {
                errors.push(ErrorInfo {
                    message: msg,
                    file: None,
                    line: None,
                    column: None,
                });
            }
        } else if let Some(caps) = warning_re.captures(line) {
            let msg = caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
            if !msg.is_empty() {
                warnings.push(WarningInfo {
                    message: msg,
                    file: None,
                    line: None,
                    column: None,
                });
            }
        }
    }

    let command_str = format!("{} {}", pm, args.join(" "));

    Ok(BuildOutput {
        success,
        command: command_str,
        package_manager: pm.to_string(),
        duration_secs: duration,
        output: combined,
        errors,
        warnings,
    })
}
