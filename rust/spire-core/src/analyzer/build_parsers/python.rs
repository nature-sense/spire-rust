// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Python build system parser (pyproject.toml, setup.py, setup.cfg).

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a Python build config file and return normalized BuildMetadata.
pub fn parse_python(project_root: &Path, build_file: &str) -> Option<BuildMetadata> {
    match build_file {
        "pyproject.toml" => parse_pyproject_toml(project_root),
        "setup.py" => parse_setup_py(project_root),
        "setup.cfg" => parse_setup_cfg(project_root),
        _ => None,
    }
}

/// Parse pyproject.toml (PEP 621 / Poetry / PDM).
fn parse_pyproject_toml(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("pyproject.toml");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;
    let mut scripts = Vec::new();
    let mut build_backend = None;

    let mut in_project = false;
    let mut in_build_system = false;
    let mut in_tool_poetry = false;
    let mut in_tool_pdm = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[project]" {
            in_project = true;
            in_build_system = false;
            in_tool_poetry = false;
            in_tool_pdm = false;
            continue;
        }
        if trimmed == "[build-system]" {
            in_project = false;
            in_build_system = true;
            in_tool_poetry = false;
            in_tool_pdm = false;
            continue;
        }
        if trimmed == "[tool.poetry]" {
            in_project = false;
            in_build_system = false;
            in_tool_poetry = true;
            in_tool_pdm = false;
            continue;
        }
        if trimmed == "[tool.pdm]" {
            in_project = false;
            in_build_system = false;
            in_tool_poetry = false;
            in_tool_pdm = true;
            continue;
        }

        if in_project {
            if let Some(val) = trimmed.strip_prefix("name = ") {
                name = Some(val.trim_matches('"').trim_matches('\'').to_string());
            }
            if let Some(val) = trimmed.strip_prefix("version = ") {
                version = Some(val.trim_matches('"').trim_matches('\'').to_string());
            }
        }

        if in_build_system {
            if let Some(val) = trimmed.strip_prefix("build-backend = ") {
                build_backend = Some(val.trim_matches('"').trim_matches('\'').to_string());
            }
        }

        if in_tool_poetry {
            if let Some(val) = trimmed.strip_prefix("name = ") {
                name = Some(val.trim_matches('"').trim_matches('\'').to_string());
            }
            if let Some(val) = trimmed.strip_prefix("version = ") {
                version = Some(val.trim_matches('"').trim_matches('\'').to_string());
            }
        }

        if in_tool_pdm {
            if let Some(val) = trimmed.strip_prefix("name = ") {
                name = Some(val.trim_matches('"').trim_matches('\'').to_string());
            }
            if let Some(val) = trimmed.strip_prefix("version = ") {
                version = Some(val.trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }

    // Detect build system
    let build_system = if build_backend.as_deref().unwrap_or("").contains("poetry") {
        "Poetry"
    } else if build_backend.as_deref().unwrap_or("").contains("pdm") {
        "PDM"
    } else if build_backend.as_deref().unwrap_or("").contains("flit") {
        "Flit"
    } else if build_backend.as_deref().unwrap_or("").contains("hatchling") {
        "Hatch"
    } else if build_backend.as_deref().unwrap_or("").contains("setuptools") {
        "setuptools"
    } else {
        "Python"
    };

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: "python_project".to_string(),
        build_system: build_system.to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts,
        features: vec![],
        targets: vec![],
        config_files: vec!["pyproject.toml".to_string()],
        raw: None,
    })
}

/// Parse setup.py (basic name/version extraction).
fn parse_setup_py(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("setup.py");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("name=").or_else(|| trimmed.strip_prefix("name =")) {
            name = Some(val.trim().trim_matches('"').trim_matches('\'').trim_end_matches(',').to_string());
        }
        if let Some(val) = trimmed.strip_prefix("version=").or_else(|| trimmed.strip_prefix("version =")) {
            version = Some(val.trim().trim_matches('"').trim_matches('\'').trim_end_matches(',').to_string());
        }
    }

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: "python_project".to_string(),
        build_system: "setuptools".to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts: vec![],
        features: vec![],
        targets: vec![],
        config_files: vec!["setup.py".to_string()],
        raw: None,
    })
}

/// Parse setup.cfg (basic name/version extraction).
fn parse_setup_cfg(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("setup.cfg");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;
    let mut in_metadata = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[metadata]" {
            in_metadata = true;
            continue;
        }
        if in_metadata {
            if trimmed.starts_with('[') {
                break;
            }
            if let Some(val) = trimmed.strip_prefix("name = ") {
                name = Some(val.to_string());
            }
            if let Some(val) = trimmed.strip_prefix("version = ") {
                version = Some(val.to_string());
            }
        }
    }

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: "python_project".to_string(),
        build_system: "setuptools".to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts: vec![],
        features: vec![],
        targets: vec![],
        config_files: vec!["setup.cfg".to_string()],
        raw: None,
    })
}
