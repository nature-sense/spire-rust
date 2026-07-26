//! Cargo project analysis — parse Cargo.toml and return structured BuildMetadata.
//!
//! This module is extracted from `spire-core/src/analyzer/build_parsers/cargo.rs`
//! and adapted to work as a standalone MCP server module.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    pub targets: Vec<BuildTarget>,
    pub dependencies: Vec<Dependency>,
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
pub struct Feature {
    pub name: String,
    pub description: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTarget {
    pub name: String,
    pub kind: String,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version_req: Option<String>,
    pub kind: String,
    pub source: String,
    pub source_url: Option<String>,
}

/// Parse a Cargo.toml and return structured BuildMetadata.
///
/// Uses `cargo_metadata` for rich data when available, falls back to
/// simple TOML parsing for name/version.
pub fn analyze_cargo_project(project_root: &str) -> Result<BuildMetadata, String> {
    let root = Path::new(project_root);
    let cargo_toml = root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(format!("No Cargo.toml found in {}", project_root));
    }

    // Try cargo_metadata first for rich data
    if let Some(cargo_info) = get_cargo_metadata(root) {
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

        return Ok(BuildMetadata {
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
                BuildScript {
                    name: "build".to_string(),
                    command: "cargo build".to_string(),
                    tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "debug"}})),
                },
                BuildScript {
                    name: "test".to_string(),
                    command: "cargo test".to_string(),
                    tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "test"}})),
                },
                BuildScript {
                    name: "run".to_string(),
                    command: "cargo run".to_string(),
                    tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "run"}})),
                },
                BuildScript {
                    name: "check".to_string(),
                    command: "cargo check".to_string(),
                    tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "check"}})),
                },
                BuildScript {
                    name: "clippy".to_string(),
                    command: "cargo clippy".to_string(),
                    tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "clippy"}})),
                },
                BuildScript {
                    name: "fmt".to_string(),
                    command: "cargo fmt".to_string(),
                    tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "fmt"}})),
                },
            ],
            features,
            targets,
            dependencies,
            config_files: vec!["Cargo.toml".to_string()],
            raw: Some(serde_json::to_value(&cargo_info).unwrap_or_default()),
        });
    }

    // Fallback: simple TOML parsing for name/version only
    let content = std::fs::read_to_string(&cargo_toml).map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;
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

    Ok(BuildMetadata {
        project_name: name,
        version,
        project_type: if is_workspace { "rust_workspace" } else { "rust_crate" }.to_string(),
        build_system: "Cargo".to_string(),
        is_workspace,
        workspace_members: vec![],
        scripts: vec![
            BuildScript {
                name: "build".to_string(),
                command: "cargo build".to_string(),
                tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "debug"}})),
            },
            BuildScript {
                name: "test".to_string(),
                command: "cargo test".to_string(),
                tool_call: Some(serde_json::json!({"tool": "project/build", "args": {"mode": "test"}})),
            },
        ],
        features: vec![],
        targets: vec![],
        dependencies: vec![],
        config_files: vec!["Cargo.toml".to_string()],
        raw: None,
    })
}

/// Rich Rust project metadata from `cargo metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoInfo {
    name: String,
    version: String,
    edition: Option<String>,
    authors: Vec<String>,
    license: Option<String>,
    description: Option<String>,
    rust_version: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
    readme: Option<String>,
    categories: Vec<String>,
    keywords: Vec<String>,
    publish: Option<Vec<String>>,
    dependencies: Vec<CargoDependency>,
    features: HashMap<String, Vec<String>>,
    targets: Vec<CargoTarget>,
    workspace_members: Vec<CargoWorkspaceMember>,
    workspace_resolver: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoDependency {
    name: String,
    version_req: Option<String>,
    kind: String,
    optional: bool,
    features: Vec<String>,
    source: Option<String>,
    git: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    src_path: String,
    edition: Option<String>,
    required_features: Vec<String>,
    crate_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CargoWorkspaceMember {
    name: String,
    version: String,
    path: String,
}

/// Run `cargo metadata` and parse the output into structured data.
fn get_cargo_metadata(project_root: &Path) -> Option<CargoInfo> {
    // Try using the cargo_metadata crate
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(project_root.join("Cargo.toml"))
        .exec()
        .ok()?;

    // Find the root package
    let root_id = metadata.resolve.as_ref()?.root.as_ref()?;
    let root_package = metadata.packages.iter().find(|p| &p.id == root_id)?;

    // Collect workspace members
    let workspace_members: Vec<CargoWorkspaceMember> = metadata
        .packages
        .iter()
        .filter(|p| &p.id != root_id)
        .map(|p| {
            // Find the manifest path relative to project root
            let manifest_dir = p.manifest_path.parent()?;
            let relative = pathdiff::diff_paths(manifest_dir, project_root)
                .unwrap_or_else(|| manifest_dir.to_path_buf().into());
            Some(CargoWorkspaceMember {
                name: p.name.clone(),
                version: p.version.to_string(),
                path: relative.to_string_lossy().to_string(),
            })
        })
        .flatten()
        .collect();

    // Collect dependencies
    let dependencies: Vec<CargoDependency> = root_package
        .dependencies
        .iter()
        .map(|d| {
            let kind = match d.kind {
                cargo_metadata::DependencyKind::Normal => "normal",
                cargo_metadata::DependencyKind::Development => "dev",
                cargo_metadata::DependencyKind::Build => "build",
                _ => "other",
            };
            CargoDependency {
                name: d.name.clone(),
                version_req: Some(d.req.to_string()),
                kind: kind.to_string(),
                optional: d.optional,
                features: d.features.clone(),
                source: d.source.as_ref().map(|s| s.to_string()),
                git: None,
                path: d.path.as_ref().map(|p| p.to_string()),
            }
        })
        .collect();

    // Collect targets
    let targets: Vec<CargoTarget> = root_package
        .targets
        .iter()
        .map(|t| CargoTarget {
            name: t.name.clone(),
            kind: t.kind.iter().map(|k| k.to_string()).collect(),
            src_path: t.src_path.to_string(),
            edition: Some(t.edition.to_string()),
            required_features: t.required_features.clone(),
            crate_types: t.crate_types.iter().map(|c| c.to_string()).collect(),
        })
        .collect();

    Some(CargoInfo {
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
        readme: root_package.readme.as_ref().map(|p| p.to_string()),
        categories: root_package.categories.clone(),
        keywords: root_package.keywords.clone(),
        publish: root_package.publish.as_ref().map(|p| p.iter().map(|s| s.to_string()).collect()),
        dependencies,
        features: root_package.features.clone().into_iter().collect(),
        targets,
        workspace_members,
        workspace_resolver: None,
    })
}
