// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Meson build system parser.

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a meson.build file and return normalized BuildMetadata.
pub fn parse_meson_build(project_root: &Path) -> Option<BuildMetadata> {
    let meson_path = project_root.join("meson.build");
    let content = std::fs::read_to_string(&meson_path).ok()?;

    let mut project_name = None;
    let mut version = None;
    let mut languages = Vec::new();
    let mut subprojects = Vec::new();
    let mut has_tests = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // project('name', 'c', version: '1.0')
        if trimmed.starts_with("project(") {
            // Extract project name (first argument)
            let args = trimmed
                .trim_start_matches("project(")
                .trim_end_matches(')');
            let parts: Vec<&str> = args.split(',').collect();
            if let Some(first) = parts.first() {
                let name = first.trim().trim_matches('\'').trim_matches('"');
                if !name.is_empty() {
                    project_name = Some(name.to_string());
                }
            }

            // Extract languages
            for part in &parts[1..] {
                let p = part.trim();
                if !p.contains(':') && !p.contains('=') {
                    let lang = p.trim_matches('\'').trim_matches('"');
                    if !lang.is_empty() {
                        languages.push(lang.to_string());
                    }
                }
            }

            // Extract version
            for part in &parts[1..] {
                let p = part.trim();
                if let Some(v) = p.strip_prefix("version:") {
                    version = Some(v.trim().trim_matches('\'').trim_matches('"').to_string());
                }
            }
        }

        // Detect subprojects
        if trimmed.starts_with("subproject(") {
            let args = trimmed
                .trim_start_matches("subproject(")
                .trim_end_matches(')');
            let name = args.trim().trim_matches('\'').trim_matches('"');
            if !name.is_empty() {
                subprojects.push(name.to_string());
            }
        }

        // Detect tests
        if trimmed.starts_with("test(") {
            has_tests = true;
        }
    }

    // Check for subprojects directory
    let subproj_dir = project_root.join("subprojects");
    if subproj_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&subproj_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.path().is_dir() && !subprojects.contains(&name) {
                    subprojects.push(name);
                }
            }
        }
    }

    // Check for wrap files
    let wrap_dir = project_root.join("subprojects");
    let mut wrap_files = Vec::new();
    if wrap_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&wrap_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".wrap") {
                    wrap_files.push(name);
                }
            }
        }
    }

    let mut scripts = Vec::new();
    scripts.push(BuildScript {
        name: "setup".to_string(),
        command: "meson setup builddir".to_string(),
    });
    scripts.push(BuildScript {
        name: "compile".to_string(),
        command: "meson compile -C builddir".to_string(),
    });
    scripts.push(BuildScript {
        name: "test".to_string(),
        command: "meson test -C builddir".to_string(),
    });
    scripts.push(BuildScript {
        name: "install".to_string(),
        command: "meson install -C builddir".to_string(),
    });

    let workspace_members: Vec<WorkspaceMember> = subprojects
        .iter()
        .map(|s| WorkspaceMember {
            name: s.clone(),
            path: format!("subprojects/{}", s),
            version: None,
        })
        .collect();

    Some(BuildMetadata {
        project_name,
        version,
        project_type: "meson_project".to_string(),
        build_system: "Meson".to_string(),
        is_workspace: !subprojects.is_empty(),
        workspace_members,
        scripts,
        features: vec![],
        targets: vec![],
        config_files: vec!["meson.build".to_string()],
        raw: None,
    })
}
