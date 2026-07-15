// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Rust project analyzer — runs `cargo metadata` for rich, resolved metadata.
//!
//! This runs `cargo metadata --no-deps` in the project root, which gives us
//! the fully resolved build model — workspace inheritance resolved, features
//! unified, all targets, dependencies with their actual resolved versions.

use std::path::Path;

use cargo_metadata::MetadataCommand;

use crate::analyzer::models::*;

/// Analyze a Rust project using `cargo metadata` for rich, resolved metadata.
///
/// Returns `None` if `cargo metadata` fails (e.g. no Cargo.toml, or the
/// project isn't a valid Cargo package).
pub fn analyze_rust_project(project_root: &Path) -> Option<CargoInfo> {
    let manifest_path = project_root.join("Cargo.toml");
    if !manifest_path.exists() {
        return None;
    }

    // Run cargo metadata --no-deps to avoid downloading dependencies
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .no_deps()
        .exec()
        .ok()?;

    // Find the root package by matching the manifest path
    let canonical_manifest = manifest_path.canonicalize().ok()?;
    let root_package = metadata.packages.iter().find(|p| {
        p.manifest_path.as_std_path().canonicalize().ok().as_deref() == Some(&canonical_manifest)
    })?;

    // Convert BTreeMap to HashMap for features
    let features: std::collections::HashMap<String, Vec<String>> = root_package
        .features
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Build the CargoInfo from the metadata
    let mut info = CargoInfo {
        name: root_package.name.clone(),
        version: root_package.version.to_string(),
        edition: Some(root_package.edition.to_string()),
        authors: root_package.authors.clone(),
        license: root_package.license.clone(),
        description: root_package.description.clone(),
        rust_version: root_package.rust_version.as_ref().map(|v| v.to_string()),
        repository: root_package.repository.clone(),
        homepage: root_package.homepage.clone(),
        documentation: root_package.documentation.clone(),
        readme: root_package.readme.as_ref().map(|r| r.to_string()),
        categories: root_package.categories.clone(),
        keywords: root_package.keywords.clone(),
        publish: root_package.publish.clone(),
        dependencies: Vec::new(),
        features,
        targets: Vec::new(),
        workspace_members: Vec::new(),
        workspace_resolver: None,
    };

    // Extract dependencies
    for dep in &root_package.dependencies {
        let kind = match dep.kind {
            cargo_metadata::DependencyKind::Normal => "normal",
            cargo_metadata::DependencyKind::Development => "dev",
            cargo_metadata::DependencyKind::Build => "build",
            _ => "platform",
        };

        let source = if dep.source.is_none() {
            None
        } else {
            dep.source.as_ref().map(|s| {
                if s.starts_with("git+") {
                    "git"
                } else if s.starts_with("path+") {
                    "path"
                } else {
                    "registry"
                }
                .to_string()
            })
        };

        info.dependencies.push(CargoDependency {
            name: dep.name.clone(),
            version_req: dep.req.to_string().into(),
            kind: kind.to_string(),
            optional: dep.optional,
            features: dep.features.clone(),
            source,
            git: None, // --no-deps doesn't resolve git URLs
            path: dep.path.as_ref().map(|p| p.to_string()),
        });
    }

    // Extract targets
    for target in &root_package.targets {
        let kind: Vec<String> = target.kind.iter().map(|k| k.to_string()).collect();
        let crate_types: Vec<String> = target.crate_types.iter().map(|c| c.to_string()).collect();

        info.targets.push(CargoTarget {
            name: target.name.clone(),
            kind,
            src_path: target.src_path.to_string(),
            edition: Some(target.edition.to_string()),
            required_features: target.required_features.clone(),
            crate_types,
        });
    }

    // Extract workspace info
    let ws_root = metadata.workspace_root.as_std_path();
    if ws_root == project_root || ws_root.canonicalize().ok().as_deref() == Some(project_root) {
        // This is the workspace root — collect workspace members
        for pkg in &metadata.packages {
            // Skip the root package itself
            if pkg.id == root_package.id {
                continue;
            }
            // Get path relative to workspace root
            let pkg_path = pkg.manifest_path.parent()?;
            let rel_path = pkg_path
                .strip_prefix(ws_root)
                .unwrap_or(pkg_path)
                .to_string();

            info.workspace_members.push(CargoWorkspaceMember {
                name: pkg.name.clone(),
                version: pkg.version.to_string(),
                path: rel_path,
            });
        }

        // Try to get workspace resolver from the workspace Cargo.toml
        let ws_cargo = ws_root.join("Cargo.toml");
        if let Ok(content) = std::fs::read_to_string(&ws_cargo) {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(val) = trimmed.strip_prefix("resolver = \"") {
                    info.workspace_resolver = Some(val.trim_end_matches('"').to_string());
                    break;
                } else if let Some(val) = trimmed.strip_prefix("resolver = '") {
                    info.workspace_resolver = Some(val.trim_end_matches('\'').to_string());
                    break;
                }
            }
        }
    }

    Some(info)
}
