use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// Universal Project File Tree — a complete, semantically-enriched view of any
// project, populated by parsing build files from any build system.
// ═══════════════════════════════════════════════════════════════════════════════

/// The complete universal project model.
///
/// This is the top-level output of the project analyzer. It contains:
/// - A full recursive file tree (`root`)
/// - Normalized build metadata from whichever build system was detected
/// - A dependency graph
/// - Semantic role annotations for every file and directory
/// - Language summary
/// - Entry points
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFileTree {
    /// The root directory node containing the entire file tree.
    pub root: DirectoryNode,
    /// Normalized build metadata (from any build system).
    pub build: BuildMetadata,
    /// Dependency graph (normalized across all build systems).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<Dependency>,
    /// Semantic role annotations for directories (path → role).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub directory_roles: HashMap<String, String>,
    /// Entry points identified.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<EntryPoint>,
    /// Language summary (sorted by file count descending).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<LanguageInfo>,
    /// Human-readable summary.
    pub summary: String,
}

/// A node in the recursive file tree — either a directory or a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FileTreeNode {
    #[serde(rename = "directory")]
    Directory(DirectoryNode),
    #[serde(rename = "file")]
    File(FileNode),
}

/// A directory in the file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryNode {
    /// Directory name (last component of path).
    pub name: String,
    /// Relative path from project root.
    pub path: String,
    /// Semantic role (e.g. "source_code", "tests", "documentation", "config").
    pub role: String,
    /// Subdirectories.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directories: Vec<DirectoryNode>,
    /// Files in this directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileNode>,
    /// Total file count (including subdirectories recursively).
    pub total_file_count: usize,
    /// Total estimated lines of code (including subdirectories recursively).
    pub total_lines: usize,
}

/// A file in the file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    /// File name.
    pub name: String,
    /// Relative path from project root.
    pub path: String,
    /// File extension (e.g. ".rs", ".ts", ".md").
    pub extension: String,
    /// Detected language (e.g. "Rust", "TypeScript", "Markdown").
    pub language: String,
    /// File size in bytes.
    pub size: u64,
    /// Estimated lines of code.
    pub lines_estimated: usize,
    /// Semantic role (e.g. "source", "entry_point", "config", "test", "build_file").
    pub role: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Normalized Build Metadata — populated by any build system parser
// ═══════════════════════════════════════════════════════════════════════════════

/// Normalized build metadata extracted from any build system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Project name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    /// Project version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Detected project type (e.g. "rust_crate", "node_package", "meson_project").
    pub project_type: String,
    /// Build system name (e.g. "Cargo", "npm", "Meson", "Gradle").
    pub build_system: String,
    /// Whether this is a workspace / multi-project root.
    pub is_workspace: bool,
    /// Workspace members (sub-projects).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_members: Vec<WorkspaceMember>,
    /// Build scripts / tasks (normalized from any build system).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<BuildScript>,
    /// Feature flags / build options.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<Feature>,
    /// Build targets (binaries, libraries, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<BuildTarget>,
    /// Config files that were parsed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_files: Vec<String>,
    /// Raw build system metadata (for advanced use).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

/// A workspace member (sub-project).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMember {
    /// Member name.
    pub name: String,
    /// Relative path from workspace root.
    pub path: String,
    /// Version (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// A build script / task (normalized).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildScript {
    /// Script name (e.g. "build", "test", "run", custom name).
    pub name: String,
    /// Command string.
    pub command: String,
}

/// A feature flag or build option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    /// Feature name.
    pub name: String,
    /// Description (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this feature is enabled by default.
    pub default: bool,
}

/// A build target (binary, library, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTarget {
    /// Target name.
    pub name: String,
    /// Target kind (e.g. "bin", "lib", "example", "test", "shared_library", "static_library").
    pub kind: String,
    /// Source file path (relative to project root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// A dependency (normalized across all build systems).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Dependency name.
    pub name: String,
    /// Version requirement string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_req: Option<String>,
    /// Dependency kind: "normal", "dev", "build", "optional", "peer".
    pub kind: String,
    /// Source type: "registry", "git", "path", "system", "wrap" (Meson wrap).
    pub source: String,
    /// Source URL (for registry/git dependencies).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Legacy types (kept for backward compatibility)
// ═══════════════════════════════════════════════════════════════════════════════

/// The complete three-stage analysis result for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectAnalysis {
    /// Absolute path to the project root.
    pub root: String,
    /// Detected project type (e.g. "rust_workspace", "node_package", "python_project").
    pub project_type: String,
    /// Confidence score (0.0–1.0) for the project type detection.
    pub confidence: f64,
    /// Languages detected in the project.
    pub languages: Vec<LanguageInfo>,
    /// Build tools detected in the project.
    pub build_tools: Vec<BuildToolInfo>,
    /// Entry points (main files, library roots, etc.).
    pub entry_points: Vec<EntryPoint>,
    /// High-level directory structure summary.
    pub directory_structure: HashMap<String, DirEntry>,
    /// Key files with their roles.
    pub key_files: Vec<KeyFile>,
    /// Human-readable summary of the project.
    pub summary: String,
    /// Recursive analyses of sub-projects (workspace members, monorepo packages, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_projects: Vec<ProjectAnalysis>,
    /// Stage 1: discovered build projects (flat list).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_projects: Vec<BuildProject>,
    /// Stage 3: remaining directories not claimed by any build project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub misc_directories: Vec<MiscDirectory>,
}

/// A build project discovered during Stage 1 scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildProject {
    /// Relative path from the project root to this project's root directory.
    pub root: String,
    /// The build config file that identified this project (e.g. "Cargo.toml", "package.json").
    pub build_file: String,
    /// Detected project type (e.g. "rust_crate", "vscode_extension", "node_package").
    pub project_type: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f64,
    /// Project name extracted from the build file (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Version extracted from the build file (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Whether this project is a workspace that contains sub-projects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_workspace: Option<bool>,
    /// Workspace member paths (relative to this project's root).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_members: Vec<String>,
    /// Languages detected in this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<LanguageInfo>,
    /// Entry points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<EntryPoint>,
    /// Key files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_files: Vec<KeyFile>,
    /// Sub-projects (recursive).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_projects: Vec<BuildProject>,
    /// Rich Cargo metadata (only for Rust projects).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_info: Option<CargoInfo>,
}

/// A directory not claimed by any build project, classified by role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiscDirectory {
    /// Relative path from the project root.
    pub path: String,
    /// Classified role (e.g. "documentation", "build_scripts", "configuration").
    pub role: String,
    /// Number of files in this directory.
    pub file_count: usize,
    /// Languages detected in this directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<LanguageInfo>,
}

/// Information about a detected language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub name: String,
    pub extensions: Vec<String>,
    pub file_count: usize,
    pub estimated_lines: usize,
}

/// Information about a detected build tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildToolInfo {
    pub name: String,
    pub config_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_workspace: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// An entry point in the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    pub path: String,
    pub entry_type: String,
}

/// A directory entry in the structure summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub dir_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_projects: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_src: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_tests: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_examples: Option<bool>,
}

/// A key file with its role in the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFile {
    pub path: String,
    pub role: String,
}

/// Raw file info collected during scanning.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: String,
    pub relative_path: String,
    pub extension: String,
    pub size: u64,
    pub is_dir: bool,
    #[allow(dead_code)]
    pub is_symlink: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Cargo metadata types (from cargo_metadata)
// ═══════════════════════════════════════════════════════════════════════════════

/// Rich metadata extracted from `cargo metadata` for a Rust project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoInfo {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<CargoDependency>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub features: HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<CargoTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_members: Vec<CargoWorkspaceMember>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_resolver: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoDependency {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_req: Option<String>,
    pub kind: String,
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoTarget {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_features: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crate_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoWorkspaceMember {
    pub name: String,
    pub version: String,
    pub path: String,
}
