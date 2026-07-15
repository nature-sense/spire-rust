// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Go build system parser (go.mod).

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a go.mod file and return normalized BuildMetadata.
pub fn parse_go_mod(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("go.mod");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut module_name = None;
    let mut go_version = None;
    let mut dependencies = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(name) = trimmed.strip_prefix("module ") {
            module_name = Some(name.trim().to_string());
        }

        if let Some(ver) = trimmed.strip_prefix("go ") {
            go_version = Some(ver.trim().to_string());
        }

        // Direct dependencies: `require pkg v1.2.3`
        if trimmed.starts_with("require ") {
            let rest = trimmed.trim_start_matches("require ");
            // Handle grouped requires: `require (`
            if rest == "(" {
                continue;
            }
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                dependencies.push(Dependency {
                    name: parts[0].to_string(),
                    version_req: Some(parts[1].to_string()),
                    kind: "normal".to_string(),
                    source: "registry".to_string(),
                    source_url: None,
                });
            }
        }

        // Indirect dependencies (inside require block)
        if !trimmed.starts_with("require ") && !trimmed.starts_with("exclude ")
            && !trimmed.starts_with("replace ") && !trimmed.starts_with("retract ")
            && !trimmed.starts_with("go ") && !trimmed.starts_with("module ")
            && !trimmed.starts_with('(') && !trimmed.starts_with(')')
            && trimmed.contains(' ')
        {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                // Check if it looks like a dependency line (pkg version)
                if !parts[0].starts_with("//") && !parts[0].contains('=') {
                    dependencies.push(Dependency {
                        name: parts[0].to_string(),
                        version_req: Some(parts[1].to_string()),
                        kind: "normal".to_string(),
                        source: "registry".to_string(),
                        source_url: None,
                    });
                }
            }
        }
    }

    Some(BuildMetadata {
        project_name: module_name,
        version: go_version,
        project_type: "go_module".to_string(),
        build_system: "Go".to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts: vec![
            BuildScript { name: "build".to_string(), command: "go build ./...".to_string() },
            BuildScript { name: "test".to_string(), command: "go test ./...".to_string() },
            BuildScript { name: "vet".to_string(), command: "go vet ./...".to_string() },
            BuildScript { name: "fmt".to_string(), command: "go fmt ./...".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec!["go.mod".to_string()],
        raw: None,
    })
}
