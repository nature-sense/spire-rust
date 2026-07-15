use std::path::Path;

use crate::models::*;

/// Parse a Python project build file and return normalized BuildMetadata.
pub fn parse_python(project_root: &Path, build_file: &str) -> Option<BuildMetadata> {
    match build_file {
        "pyproject.toml" => parse_pyproject_toml(project_root),
        "setup.py" => parse_setup_py(project_root),
        "setup.cfg" => parse_setup_cfg(project_root),
        _ => None,
    }
}

/// Parse pyproject.toml (PEP 621 / Poetry / PDM / Flit).
fn parse_pyproject_toml(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("pyproject.toml");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;
    let mut dependencies = Vec::new();
    let mut in_project = false;
    let mut in_dependencies = false;
    let mut in_build_system = false;
    let mut build_backend = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[project]" {
            in_project = true;
            in_dependencies = false;
            in_build_system = false;
            continue;
        }
        if trimmed == "[build-system]" {
            in_build_system = true;
            in_project = false;
            in_dependencies = false;
            continue;
        }
        if trimmed.starts_with("[tool.") {
            in_project = false;
            in_dependencies = false;
            in_build_system = false;
            continue;
        }
        if trimmed.starts_with('[') {
            in_project = false;
            in_dependencies = false;
            in_build_system = false;
            continue;
        }

        if in_project {
            if let Some(val) = trimmed.strip_prefix("name = ") {
                name = Some(val.trim_matches('"').to_string());
            }
            if let Some(val) = trimmed.strip_prefix("version = ") {
                version = Some(val.trim_matches('"').to_string());
            }
            if trimmed == "dependencies = [" || trimmed.starts_with("dependencies = [") {
                in_dependencies = true;
                // Parse inline deps
                if let Some(deps_content) = trimmed.strip_prefix("dependencies = [") {
                    if let Some(close) = deps_content.find(']') {
                        let deps_str = &deps_content[..close];
                        for dep in deps_str.split(',') {
                            let clean = dep.trim().trim_matches('"').trim_matches('\'');
                            if !clean.is_empty() {
                                let dep_name = clean.split(&['>', '<', '=', '!', '~', '@'][..]).next().unwrap_or(clean).trim().to_string();
                                dependencies.push(Dependency {
                                    name: dep_name,
                                    version_req: None,
                                    kind: "normal".to_string(),
                                    source: "registry".to_string(),
                                    source_url: None,
                                });
                            }
                        }
                        in_dependencies = false;
                    }
                }
                continue;
            }
            if in_dependencies {
                if trimmed == "]" {
                    in_dependencies = false;
                    continue;
                }
                let clean = trimmed.trim_matches(',').trim_matches('"').trim_matches('\'');
                if !clean.is_empty() {
                    let dep_name = clean.split(&['>', '<', '=', '!', '~', '@'][..]).next().unwrap_or(clean).trim().to_string();
                    dependencies.push(Dependency {
                        name: dep_name,
                        version_req: None,
                        kind: "normal".to_string(),
                        source: "registry".to_string(),
                        source_url: None,
                    });
                }
            }
        }

        if in_build_system {
            if let Some(val) = trimmed.strip_prefix("build-backend = ") {
                build_backend = Some(val.trim_matches('"').to_string());
            }
        }
    }

    // Detect build system from build-backend
    let build_system = match build_backend.as_deref() {
        Some(b) if b.contains("poetry") => "Poetry",
        Some(b) if b.contains("pdm") => "PDM",
        Some(b) if b.contains("flit") => "Flit",
        Some(b) if b.contains("setuptools") => "setuptools",
        _ => "Python (pyproject.toml)",
    };

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: "python_project".to_string(),
        build_system: build_system.to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts: vec![
            BuildScript { name: "install".to_string(), command: "pip install .".to_string() },
            BuildScript { name: "test".to_string(), command: "pytest".to_string() },
            BuildScript { name: "lint".to_string(), command: "ruff check .".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec!["pyproject.toml".to_string()],
        raw: None,
    })
}

/// Parse setup.py (legacy Python packaging).
fn parse_setup_py(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("setup.py");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("name=").or_else(|| trimmed.strip_prefix("name = ")) {
            name = Some(val.trim().trim_matches('"').trim_matches('\'').trim_matches(',').to_string());
        }
        if let Some(val) = trimmed.strip_prefix("version=").or_else(|| trimmed.strip_prefix("version = ")) {
            version = Some(val.trim().trim_matches('"').trim_matches('\'').trim_matches(',').to_string());
        }
    }

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: "python_project".to_string(),
        build_system: "setuptools".to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts: vec![
            BuildScript { name: "install".to_string(), command: "pip install -e .".to_string() },
            BuildScript { name: "test".to_string(), command: "pytest".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec!["setup.py".to_string()],
        raw: None,
    })
}

/// Parse setup.cfg (legacy Python packaging).
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
        if trimmed.starts_with('[') {
            in_metadata = false;
            continue;
        }
        if in_metadata {
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
        scripts: vec![
            BuildScript { name: "install".to_string(), command: "pip install -e .".to_string() },
            BuildScript { name: "test".to_string(), command: "pytest".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec!["setup.cfg".to_string()],
        raw: None,
    })
}
