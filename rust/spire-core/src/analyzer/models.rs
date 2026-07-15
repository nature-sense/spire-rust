// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Data models for the project analyzer.
//!
//! These types represent the full semantic structure of a project:
//! - [`FileInfo`]: A single file or directory entry from scanning.
//! - [`DirectoryNode`]: A directory in the hierarchical tree.
//! - [`FileNode`]: A file in the hierarchical tree.
//! - [`ProjectFileTree`]: The complete analysis result.
//! - [`BuildMetadata`]: Normalized build system metadata.
//! - [`LanguageInfo`]: Language statistics.

use serde::{Deserialize, Serialize};

/// A single file or directory entry from scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    /// Absolute path to the file.
    pub path: String,
    /// Path relative to the scan root.
    pub relative_path: String,
    /// File extension (e.g. ".rs", ".ts", "" for no extension).
    pub extension: String,
    /// File size in bytes.
    pub size: u64,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Whether this entry is a symlink.
    pub is_symlink: bool,
}

/// A directory in the hierarchical file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryNode {
    /// Directory name (last component).
    pub name: String,
    /// Path relative to the project root.
    pub path: String,
    /// Semantic role (e.g. "source_code", "tests", "documentation").
    pub role: String,
    /// Subdirectories.
    pub directories: Vec<DirectoryNode>,
    /// Files in this directory.
    pub files: Vec<FileNode>,
    /// Total number of files in this directory (recursive).
    pub total_file_count: usize,
    /// Total estimated lines of code (recursive).
    pub total_lines: usize,
}

/// A file in the hierarchical file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    /// File name (last component).
    pub name: String,
    /// Path relative to the project root.
    pub path: String,
    /// File extension (e.g. ".rs", ".ts").
    pub extension: String,
    /// Detected programming language.
    pub language: String,
    /// File size in bytes.
    pub size: u64,
    /// Estimated lines of code.
    pub lines_estimated: usize,
    /// Semantic role (e.g. "source", "test", "config", "documentation").
    pub role: String,
}

/// The complete result of a project analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFileTree {
    /// Root directory node (the project root).
    pub root: DirectoryNode,
    /// Merged build metadata from all build config files.
    pub build: BuildMetadata,
    /// Detected languages, sorted by file count descending.
    pub languages: Vec<LanguageInfo>,
}

/// Normalized build system metadata.
///
/// This is the universal representation of any build system's configuration.
/// Each build system parser (Cargo, npm, Meson, etc.) converts its native
/// format into this structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Project name (if available).
    pub project_name: Option<String>,
    /// Project version (if available).
    pub version: Option<String>,
    /// Detected project type (e.g. "rust_crate", "node_package", "meson_project").
    pub project_type: String,
    /// Build system name (e.g. "Cargo", "npm", "Meson").
    pub build_system: String,
    /// Whether this is a workspace/multi-module project.
    pub is_workspace: bool,
    /// Workspace member paths (if applicable).
    pub workspace_members: Vec<WorkspaceMember>,
    /// Available build scripts/targets.
    pub scripts: Vec<BuildScript>,
    /// Feature flags (if applicable).
    pub features: Vec<Feature>,
    /// Build targets (executables, libraries, etc.).
    pub targets: Vec<BuildTarget>,
    /// Build config file paths.
    pub config_files: Vec<String>,
    /// Raw parsed data (build-system-specific).
    pub raw: Option<serde_json::Value>,
}

/// A workspace member in a multi-module project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMember {
    /// Member name.
    pub name: String,
    /// Path relative to workspace root.
    pub path: String,
    /// Version (if available).
    pub version: Option<String>,
}

/// A build script or task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildScript {
    /// Script name (e.g. "build", "test").
    pub name: String,
    /// Shell command to run.
    pub command: String,
}

/// A feature flag or build option.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    /// Feature name.
    pub name: String,
    /// Description (if available).
    pub description: Option<String>,
    /// Whether this feature is enabled by default.
    pub default: bool,
}

/// A build target (executable, library, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildTarget {
    /// Target name.
    pub name: String,
    /// Target kind (e.g. "bin", "lib", "executable", "test").
    pub kind: String,
    /// Source file path (if available).
    pub source_path: Option<String>,
}

/// A dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Dependency name.
    pub name: String,
    /// Version requirement string.
    pub version_req: Option<String>,
    /// Dependency kind (e.g. "normal", "dev", "build").
    pub kind: String,
    /// Source type (e.g. "registry", "git", "path", "system", "wrap").
    pub source: String,
    /// Source URL (if applicable).
    pub source_url: Option<String>,
}

/// Language statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    /// Language name (e.g. "Rust", "TypeScript").
    pub name: String,
    /// File extensions associated with this language.
    pub extensions: Vec<String>,
    /// Number of files in this language.
    pub file_count: usize,
    /// Estimated total lines of code.
    pub estimated_lines: usize,
}

// ── Rust-specific metadata (from cargo_metadata) ──────────────────────────

/// Rich Rust project metadata from `cargo metadata`.
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
    pub publish: Option<Vec<String>>,
    pub dependencies: Vec<CargoDependency>,
    pub features: std::collections::HashMap<String, Vec<String>>,
    pub targets: Vec<CargoTarget>,
    pub workspace_members: Vec<CargoWorkspaceMember>,
    pub workspace_resolver: Option<String>,
}

/// A dependency from Cargo.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoDependency {
    pub name: String,
    pub version_req: Option<String>,
    pub kind: String,
    pub optional: bool,
    pub features: Vec<String>,
    pub source: Option<String>,
    pub git: Option<String>,
    pub path: Option<String>,
}

/// A build target from Cargo.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoTarget {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: String,
    pub edition: Option<String>,
    pub required_features: Vec<String>,
    pub crate_types: Vec<String>,
}

/// A workspace member from Cargo.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoWorkspaceMember {
    pub name: String,
    pub version: String,
    pub path: String,
}
