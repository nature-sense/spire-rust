//! Node.js project analysis — parse package.json and return structured BuildMetadata.
//!
//! This module is extracted from `spire-core/src/analyzer/build_parsers/node.rs`
//! and adapted to work as a standalone MCP server module.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Normalized build system metadata (mirrors spire-core's BuildMetadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    pub project_name: Option<String>,
    pub version: Option<String>,
    pub project_type: String,
    pub build_system: String,
    pub is_workspace: bool,
    pub workspace_members: Vec<WorkspaceMember>,
    pub scripts: Vec<BuildScript>,
    pub features: Vec<Feature>,
    pub dependencies: Vec<Dependency>,
    pub targets: Vec<BuildTarget>,
    pub config_files: Vec<String>,
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMember {
    pub name: String,
    pub path: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildScript {
    pub name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTarget {
    pub name: String,
    pub kind: String,
    pub source_path: Option<String>,
}

/// A feature flag or build option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub name: String,
    pub description: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version_req: Option<String>,
    pub kind: String,
    pub source: String,
    pub source_url: Option<String>,
}

/// Parse a package.json and return structured BuildMetadata.
pub fn analyze_node_project(project_root: &str) -> Result<BuildMetadata, String> {
    let root = Path::new(project_root);

    let pkg_path = root.join("package.json");
    let content = std::fs::read_to_string(&pkg_path)
        .map_err(|e| format!("Failed to read package.json: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Invalid package.json: {e}"))?;

    let name = json.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
    let version = json.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Detect build system
    let has_pnpm_lock = root.join("pnpm-lock.yaml").exists();
    let has_yarn_lock = root.join("yarn.lock").exists();
    let has_npm_lock = root.join("package-lock.json").exists();
    let has_pnpm_workspace = root.join("pnpm-workspace.yaml").exists();

    let build_system = if has_pnpm_lock || has_pnpm_workspace {
        "pnpm"
    } else if has_yarn_lock {
        "yarn"
    } else {
        "npm"
    };

    // Detect project type
    let is_vscode_ext = json.get("contributes").is_some()
        || json.get("activationEvents").is_some()
        || json.get("engines").and_then(|e| e.get("vscode")).is_some();

    let project_type = if is_vscode_ext {
        "vscode_extension"
    } else if has_pnpm_workspace {
        "pnpm_workspace"
    } else {
        "node_package"
    };

    // Parse scripts
    let build_system_name = build_system; // capture for closure
    let scripts: Vec<BuildScript> = json
        .get("scripts")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(name, cmd)| {
                    let raw_cmd = cmd.as_str().unwrap_or("").to_string();
                    BuildScript {
                        name: name.clone(),
                        command: raw_cmd.clone(),
                        tool_call: Some(serde_json::json!({
                            "tool": "project/build",
                            "args": {
                                "mode": name.clone(),
                                "command": raw_cmd,
                                "buildSystem": build_system_name,
                            }
                        })),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Parse dependencies
    let mut dependencies = Vec::new();
    if let Some(deps) = json.get("dependencies").and_then(|v| v.as_object()) {
        for (name, ver) in deps {
            dependencies.push(Dependency {
                name: name.clone(),
                version_req: ver.as_str().map(|s| s.to_string()),
                kind: "normal".to_string(),
                source: "registry".to_string(),
                source_url: None,
            });
        }
    }
    if let Some(deps) = json.get("devDependencies").and_then(|v| v.as_object()) {
        for (name, ver) in deps {
            dependencies.push(Dependency {
                name: name.clone(),
                version_req: ver.as_str().map(|s| s.to_string()),
                kind: "dev".to_string(),
                source: "registry".to_string(),
                source_url: None,
            });
        }
    }

    // Detect workspace members
    let workspace_members = detect_workspace_members(root, &json);

    // Detect entry points from "main" and "bin" fields
    let mut targets = Vec::new();
    if let Some(main) = json.get("main").and_then(|v| v.as_str()) {
        targets.push(BuildTarget {
            name: name.clone().unwrap_or_else(|| "main".to_string()),
            kind: "lib".to_string(),
            source_path: Some(main.to_string()),
        });
    }
    if let Some(bin) = json.get("bin") {
        if let Some(bin_str) = bin.as_str() {
            targets.push(BuildTarget {
                name: name.clone().unwrap_or_else(|| "bin".to_string()),
                kind: "bin".to_string(),
                source_path: Some(bin_str.to_string()),
            });
        } else if let Some(bin_obj) = bin.as_object() {
            for (bin_name, bin_path) in bin_obj {
                targets.push(BuildTarget {
                    name: bin_name.clone(),
                    kind: "bin".to_string(),
                    source_path: bin_path.as_str().map(|s| s.to_string()),
                });
            }
        }
    }

    let mut config_files = vec!["package.json".to_string()];
    if has_pnpm_lock {
        config_files.push("pnpm-lock.yaml".to_string());
    }
    if has_pnpm_workspace {
        config_files.push("pnpm-workspace.yaml".to_string());
    }
    if has_yarn_lock {
        config_files.push("yarn.lock".to_string());
    }
    if has_npm_lock {
        config_files.push("package-lock.json".to_string());
    }

    Ok(BuildMetadata {
        project_name: name,
        version,
        project_type: project_type.to_string(),
        build_system: build_system.to_string(),
        is_workspace: has_pnpm_workspace || !workspace_members.is_empty(),
        workspace_members,
        scripts,
        features: vec![],
        dependencies,
        targets,
        config_files,
        raw: Some(json),
    })

}

/// Detect workspace members from package.json workspaces field.
fn detect_workspace_members(project_root: &Path, json: &serde_json::Value) -> Vec<WorkspaceMember> {
    let mut members = Vec::new();

    let workspaces = json.get("workspaces");
    if workspaces.is_none() {
        return members;
    }
    let workspaces = workspaces.unwrap();

    let patterns: Vec<&str> = if let Some(arr) = workspaces.as_array() {
        arr.iter().filter_map(|v| v.as_str()).collect()
    } else if let Some(obj) = workspaces.as_object() {
        obj.get("packages")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    } else {
        return members;
    };

    for pattern in patterns {
        let glob_path = project_root.join(pattern);
        if !pattern.contains('*') && !pattern.contains('?') {
            if glob_path.is_dir() {
                if let Some(name) = glob_path.file_name().map(|n| n.to_string_lossy().to_string()) {
                    members.push(WorkspaceMember {
                        name,
                        path: pattern.to_string(),
                        version: None,
                    });
                }
            }
        } else {
            if let Some(parent) = glob_path.parent() {
                if let Ok(entries) = std::fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let entry_name = entry.file_name().to_string_lossy().to_string();
                        if simple_glob_match(pattern, &entry_name) && entry.path().is_dir() {
                            let en = entry_name.clone();
                            members.push(WorkspaceMember {
                                name: entry_name,
                                path: format!("{}/{}", pattern.trim_end_matches('*'), en),
                                version: None,
                            });
                        }
                    }
                }
            }
        }
    }

    members
}

/// Simple glob matching (supports * and ?).
fn simple_glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        if pattern.contains('?') {
            if pattern.len() != name.len() {
                return false;
            }
            for (p, n) in pattern.chars().zip(name.chars()) {
                if p != '?' && p != n {
                    return false;
                }
            }
            return true;
        }
        return pattern == name;
    }
    if !parts[0].is_empty() && !name.starts_with(parts[0]) {
        return false;
    }
    if !parts.last().unwrap_or(&"").is_empty() && !name.ends_with(parts.last().unwrap_or(&"")) {
        return false;
    }
    let mut pos = parts[0].len();
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = name[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }
    true
}
