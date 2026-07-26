use serde::{Deserialize, Serialize};

/// A single file or directory entry from scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub relative_path: String,
    pub extension: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// A directory in the hierarchical file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryNode {
    pub name: String,
    pub path: String,
    pub role: String,
    pub directories: Vec<DirectoryNode>,
    pub files: Vec<FileNode>,
    pub total_file_count: usize,
    pub total_lines: usize,
}

/// A file in the hierarchical file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub extension: String,
    pub language: String,
    pub size: u64,
    pub lines_estimated: usize,
    pub role: String,
}

/// The complete result of a project analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFileTree {
    pub root: DirectoryNode,
    pub build: BuildMetadata,
    pub languages: Vec<LanguageInfo>,
    /// Detected packaging information (e.g. how to build and distribute).
    pub packaging: Option<PackagingInfo>,
    /// Detected packaging information (e.g. how to build and distribute).
    pub packaging: Option<PackagingInfo>,
}

/// Normalized metadata for all detected build systems in a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Detected build system types
    pub build_types: Vec<String>,
    /// Build system config files found
    pub config_files: Vec<String>,
    /// Available build commands from package.json scripts (if Node.js project)
    pub node_scripts: Vec<BuildScript>,
    /// Cargo workspace members (if Rust workspace)
    pub workspace_members: Vec<String>,
    /// Entry points detected (main.rs, extension.ts, etc.)
    pub entry_points: Vec<String>,
    /// Backward-compat: project name (derived from first workspace member or directory)
    pub project_name: Option<String>,
    /// Backward-compat: project type (e.g. "rust_workspace", "vscode_extension")
    pub project_type: String,
    /// Backward-compat: primary build system (e.g. "Cargo", "npm")
    pub build_system: String,
}

/// A single script entry from package.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildScript {
    pub name: String,
    pub command: String,
}

/// Packaging information for the project — how to build and distribute
/// the final artifact. Detected by analyzing package.json build configs,
/// VSCode extension manifests, and binary staging scripts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagingInfo {
    /// Type of packager (e.g. "vsce", "npm", "docker", "python-wheels")
    pub packager_type: String,
    /// Human-readable description
    pub description: String,
    /// Compiled native binaries that must be staged before packaging
    pub native_binaries: Vec<String>,
    /// The staging directory for native binaries (relative to project root)
    pub binary_staging_dir: Option<String>,
    /// The entry point for the package (e.g. extension.js main file)
    pub package_entry_point: Option<String>,
    /// Missing dependencies that would prevent packaging
    pub missing_dependencies: Vec<String>,
}

/// Language statistics from analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub name: String,
    pub file_count: usize,
    pub total_lines: usize,
    pub extensions: Vec<String>,
}

/// Rust-specific metadata from `cargo metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoInfo {
    pub name: String,
    pub version: String,
    pub edition: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub description: Option<String>,
    pub rust_version: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub readme: Option<String>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub dependencies: Vec<CargoDependency>,
    pub features: std::collections::HashMap<String, Vec<String>>,
}

/// A single dependency from `cargo metadata`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoDependency {
    pub name: String,
    pub version_req: String,
    pub optional: bool,
    pub features: Vec<String>,
}

/// Node.js-specific metadata from package.json analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    pub version: String,
    pub private: bool,
    pub scripts: Vec<BuildScript>,
    pub dependencies: Vec<String>,
    pub dev_dependencies: Vec<String>,
}

// ============================================================================
// Packaging Types
// ============================================================================

/// Packaging information for the project — how to build and distribute
/// the final artifact. Detected by analyzing package.json build configs,
/// VSCode extension manifests, and binary staging scripts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagingInfo {
    /// Type of packager (e.g. "vsce", "npm", "docker", "python-wheels")
    pub packager_type: String,
    /// Human-readable description
    pub description: String,
    /// Compiled native binaries that must be staged before packaging
    pub native_binaries: Vec<String>,
    /// The staging directory for native binaries (relative to project root)
    pub binary_staging_dir: Option<String>,
    /// The entry point for the package (e.g. extension.js main file)
    pub package_entry_point: Option<String>,
    /// Missing dependencies that would prevent packaging
    pub missing_dependencies: Vec<String>,
}
