// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Make build system parser (Makefile).

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a Makefile and return normalized BuildMetadata.
pub fn parse_makefile(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("Makefile");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut targets = Vec::new();
    let mut variables = std::collections::HashMap::new();
    let mut project_name = None;
    let mut version = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Variable assignment: VAR = value
        if let Some(eq_pos) = trimmed.find('=') {
            let var_name = trimmed[..eq_pos].trim().to_string();
            let var_value = trimmed[eq_pos + 1..].trim().to_string();
            variables.insert(var_name, var_value);
        }

        // Target definition: target: dependencies
        if trimmed.contains(':') && !trimmed.starts_with('\t') && !trimmed.starts_with(' ') {
            let colon_pos = trimmed.find(':').unwrap();
            let target_name = trimmed[..colon_pos].trim().to_string();

            // Skip pattern rules (containing %)
            if target_name.contains('%') {
                continue;
            }

            // Skip special targets
            if target_name.starts_with('.') {
                continue;
            }

            targets.push(BuildTarget {
                name: target_name.clone(),
                kind: "make_target".to_string(),
                source_path: None,
            });
        }
    }

    // Try to extract project name from common variables
    if let Some(name) = variables.get("PROJECT_NAME")
        .or_else(|| variables.get("PROJECT"))
        .or_else(|| variables.get("NAME"))
    {
        project_name = Some(name.clone());
    }

    // Try to extract version
    if let Some(ver) = variables.get("VERSION")
        .or_else(|| variables.get("PROJECT_VERSION"))
    {
        version = Some(ver.clone());
    }

    // Build scripts from common targets
    let mut scripts = Vec::new();
    let common_targets = ["all", "build", "test", "clean", "install", "run", "check", "lint", "format", "docs"];

    for target in &common_targets {
        if targets.iter().any(|t| t.name == *target) {
            scripts.push(BuildScript {
                name: target.to_string(),
                command: format!("make {}", target),
            });
        }
    }

    // If no common targets found, add the first few targets as scripts
    if scripts.is_empty() {
        for target in targets.iter().take(5) {
            scripts.push(BuildScript {
                name: target.name.clone(),
                command: format!("make {}", target.name),
            });
        }
    }

    Some(BuildMetadata {
        project_name,
        version,
        project_type: "make_project".to_string(),
        build_system: "Make".to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts,
        features: vec![],
        targets,
        config_files: vec!["Makefile".to_string()],
        raw: None,
    })
}
