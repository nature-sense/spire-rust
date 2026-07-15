use std::path::Path;

use crate::models::*;

/// Parse a package.json and return normalized BuildMetadata.
pub fn parse_package_json(project_root: &Path) -> Option<BuildMetadata> {
    let pkg_path = project_root.join("package.json");
    let content = std::fs::read_to_string(&pkg_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let name = json.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
    let version = json.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Detect build system
    let has_pnpm_lock = project_root.join("pnpm-lock.yaml").exists();
    let has_yarn_lock = project_root.join("yarn.lock").exists();
    let has_npm_lock = project_root.join("package-lock.json").exists();
    let has_pnpm_workspace = project_root.join("pnpm-workspace.yaml").exists();

    let build_system = if has_pnpm_lock || has_pnpm_workspace {
        "pnpm"
    } else if has_yarn_lock {
        "Yarn"
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
    let scripts: Vec<BuildScript> = json
        .get("scripts")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(name, cmd)| BuildScript {
                    name: name.clone(),
                    command: cmd.as_str().unwrap_or("").to_string(),
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
    let workspace_members = detect_workspace_members(project_root, &json);

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

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: project_type.to_string(),
        build_system: build_system.to_string(),
        is_workspace: has_pnpm_workspace || !workspace_members.is_empty(),
        workspace_members,
        scripts,
        features: vec![],
        targets,
        config_files,
        raw: Some(json),
    })
}

/// Parse a pnpm-workspace.yaml and return BuildMetadata for the workspace.
pub fn parse_pnpm_workspace(project_root: &Path) -> Option<BuildMetadata> {
    let yaml_path = project_root.join("pnpm-workspace.yaml");
    let content = std::fs::read_to_string(&yaml_path).ok()?;

    // Simple YAML parsing for packages array
    let mut in_packages = false;
    let mut members = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "packages:" {
            in_packages = true;
            continue;
        }
        if in_packages {
            if trimmed.starts_with('-') {
                let pattern = trimmed.trim_start_matches('-').trim().trim_matches('\'');
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
                    // Expand glob
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
            } else if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            } else {
                break;
            }
        }
    }

    Some(BuildMetadata {
        project_name: None,
        version: None,
        project_type: "pnpm_workspace".to_string(),
        build_system: "pnpm".to_string(),
        is_workspace: true,
        workspace_members: members,
        scripts: vec![],
        features: vec![],
        targets: vec![],
        config_files: vec!["pnpm-workspace.yaml".to_string()],
        raw: None,
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
