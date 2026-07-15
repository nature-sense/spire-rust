// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Cargo build system parser.

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a Cargo.toml and return normalized BuildMetadata.
///
/// Uses `cargo_metadata` for rich data when available, falls back to
/// simple TOML parsing for name/version.
pub fn parse_cargo(project_root: &Path) -> Option<BuildMetadata> {
    let cargo_toml = project_root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return None;
    }

    // Try cargo_metadata first for rich data
    if let Some(cargo_info) = crate::analyzer::rust_analyzer::analyze_rust_project(project_root) {
        let is_workspace = !cargo_info.workspace_members.is_empty();

        let workspace_members: Vec<WorkspaceMember> = cargo_info
            .workspace_members
            .iter()
            .map(|m| WorkspaceMember {
                name: m.name.clone(),
                path: m.path.clone(),
                version: Some(m.version.clone()),
            })
            .collect();

        let features: Vec<Feature> = cargo_info
            .features
            .iter()
            .map(|(name, _)| Feature {
                name: name.clone(),
                description: None,
                default: false,
            })
            .collect();

        let targets: Vec<BuildTarget> = cargo_info
            .targets
            .iter()
            .map(|t| BuildTarget {
                name: t.name.clone(),
                kind: t.kind.first().cloned().unwrap_or_else(|| "other".to_string()),
                source_path: Some(t.src_path.clone()),
            })
            .collect();

        let dependencies: Vec<Dependency> = cargo_info
            .dependencies
            .iter()
            .map(|d| {
                let source = if d.path.is_some() {
                    "path"
                } else if d.git.is_some() {
                    "git"
                } else {
                    "registry"
                };
                Dependency {
                    name: d.name.clone(),
                    version_req: d.version_req.clone(),
                    kind: d.kind.clone(),
                    source: source.to_string(),
                    source_url: d.git.clone().or_else(|| d.path.clone()),
                }
            })
            .collect();

        return Some(BuildMetadata {
            project_name: Some(cargo_info.name.clone()),
            version: Some(cargo_info.version.clone()),
            project_type: if is_workspace {
                "rust_workspace".to_string()
            } else {
                "rust_crate".to_string()
            },
            build_system: "Cargo".to_string(),
            is_workspace,
            workspace_members,
            scripts: vec![
                BuildScript { name: "build".to_string(), command: "cargo build".to_string() },
                BuildScript { name: "test".to_string(), command: "cargo test".to_string() },
                BuildScript { name: "run".to_string(), command: "cargo run".to_string() },
                BuildScript { name: "check".to_string(), command: "cargo check".to_string() },
                BuildScript { name: "clippy".to_string(), command: "cargo clippy".to_string() },
                BuildScript { name: "fmt".to_string(), command: "cargo fmt".to_string() },
            ],
            features,
            targets,
            config_files: vec!["Cargo.toml".to_string()],
            raw: Some(serde_json::to_value(&cargo_info).unwrap_or_default()),
        });
    }

    // Fallback: simple TOML parsing for name/version only
    let content = std::fs::read_to_string(&cargo_toml).ok()?;
    let mut name = None;
    let mut version = None;
    let mut in_package = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if in_package {
            if trimmed.starts_with('[') {
                break;
            }
            if let Some(val) = trimmed.strip_prefix("name = ") {
                name = Some(val.trim_matches('"').to_string());
            }
            if let Some(val) = trimmed.strip_prefix("version = ") {
                version = Some(val.trim_matches('"').to_string());
            }
        }
    }

    let is_workspace = content.contains("[workspace]");

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: if is_workspace { "rust_workspace" } else { "rust_crate" }.to_string(),
        build_system: "Cargo".to_string(),
        is_workspace,
        workspace_members: vec![],
        scripts: vec![
            BuildScript { name: "build".to_string(), command: "cargo build".to_string() },
            BuildScript { name: "test".to_string(), command: "cargo test".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec!["Cargo.toml".to_string()],
        raw: None,
    })
}
