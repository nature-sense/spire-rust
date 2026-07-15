// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! ProjectAnalyzerActor — semantic project analysis for LLM understanding.
//!
//! This actor uses the `analyzer` module to produce a structured, semantic
//! representation of a project's directory structure, build systems, languages,
//! and architecture. The output is designed to give an LLM a rich understanding
//! of the project without needing to read every file.
//!
//! # Output
//!
//! The actor produces a `ProjectAnalysis` struct containing:
//! - Full file tree with semantic role annotations
//! - Build system metadata (Cargo, npm, Python, Go, Gradle, Maven, CMake, Make, Meson)
//! - Language breakdown
//! - Directory role classification
//! - Entry points and key files
//! - Architecture summary

use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;
use tracing::{debug, info};

use crate::actors::Actor;
use crate::analyzer::models::*;
use crate::analyzer::scanner;
use crate::analyzer::tree_builder;
use crate::analyzer::build_parsers;

// ============================================================================
// ProjectAnalysis — the structured output
// ============================================================================

/// Complete semantic analysis of a project.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectAnalysis {
    /// Project root path.
    pub project_root: String,
    /// Project name (inferred from directory name or build config).
    pub project_name: String,
    /// The full file tree with semantic annotations.
    pub file_tree: DirectoryNode,
    /// Build system metadata (one per detected build system).
    pub build_systems: Vec<BuildMetadata>,
    /// Language breakdown (language → file count).
    pub languages: Vec<LanguageBreakdown>,
    /// Directory role breakdown (role → directory count).
    pub directory_roles: Vec<RoleBreakdown>,
    /// File role breakdown (role → file count).
    pub file_roles: Vec<RoleBreakdown>,
    /// Key entry points (main files, lib files, etc.).
    pub entry_points: Vec<String>,
    /// Architecture summary (human-readable).
    pub architecture_summary: String,
    /// Total file count.
    pub total_files: usize,
    /// Total directory count.
    pub total_dirs: usize,
    /// Estimated total lines of code.
    pub total_lines: usize,
}

/// Language breakdown entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LanguageBreakdown {
    pub language: String,
    pub file_count: usize,
    pub line_estimate: usize,
}

/// Role breakdown entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoleBreakdown {
    pub role: String,
    pub count: usize,
}

// ============================================================================
// ProjectAnalyzerMessage
// ============================================================================

/// Messages for the ProjectAnalyzer actor.
pub enum ProjectAnalyzerMessage {
    /// Initialize the actor with resources.
    Initialize {
        reply_to: oneshot::Sender<Result<()>>,
    },
    /// Analyze a project at the given root path.
    Analyze {
        project_root: PathBuf,
        reply_to: oneshot::Sender<Result<ProjectAnalysis>>,
    },
    /// Get a summary of the project (lighter weight than full analysis).
    Summarize {
        project_root: PathBuf,
        reply_to: oneshot::Sender<Result<String>>,
    },
}

// ============================================================================
// ProjectAnalyzerActor
// ============================================================================

/// The ProjectAnalyzer actor — semantic project analysis.
pub struct ProjectAnalyzerActor;

impl ProjectAnalyzerActor {
    pub fn new() -> Self {
        Self
    }

    /// Perform a full semantic analysis of a project.
    async fn analyze(&self, project_root: &Path) -> Result<ProjectAnalysis> {
        let start = std::time::Instant::now();
        info!("ProjectAnalyzer: analyzing project at {}", project_root.display());

        // 1. Scan the project (collect files with metadata)
        let files = scanner::scan_directory(project_root, false);
        debug!("ProjectAnalyzer: scanned {} files", files.len());

        // 2. Build the file tree with semantic annotations
        let file_tree = tree_builder::build_file_tree(project_root, false);
        debug!("ProjectAnalyzer: built file tree");

        // 3. Detect build config files and parse them
        let build_configs = scanner::discover_build_files(project_root, false);
        let mut build_systems: Vec<BuildMetadata> = Vec::new();
        for (build_file, _parent_dir) in &build_configs {
            if let Some(metadata) = build_parsers::parse_build_file(project_root, build_file, &files) {
                build_systems.push(metadata);
            }
        }
        debug!("ProjectAnalyzer: detected {} build systems", build_systems.len());

        // 4. Compute language breakdown from the file tree
        let mut lang_map: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
        collect_languages_from_tree(&file_tree, &mut lang_map);
        let mut languages: Vec<LanguageBreakdown> = lang_map
            .into_iter()
            .map(|(language, (file_count, line_estimate))| LanguageBreakdown {
                language,
                file_count,
                line_estimate,
            })
            .collect();
        languages.sort_by(|a, b| b.file_count.cmp(&a.file_count));

        // 5. Compute directory role breakdown
        let mut dir_role_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        collect_directory_roles(&file_tree, &mut dir_role_map);
        let mut directory_roles: Vec<RoleBreakdown> = dir_role_map
            .into_iter()
            .map(|(role, count)| RoleBreakdown { role, count })
            .collect();
        directory_roles.sort_by(|a, b| b.count.cmp(&a.count));

        // 6. Compute file role breakdown
        let mut file_role_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        collect_file_roles(&file_tree, &mut file_role_map);
        let mut file_roles: Vec<RoleBreakdown> = file_role_map
            .into_iter()
            .map(|(role, count)| RoleBreakdown { role, count })
            .collect();
        file_roles.sort_by(|a, b| b.count.cmp(&a.count));

        // 7. Identify entry points
        let entry_points: Vec<String> = find_entry_points(&file_tree);

        // 8. Build architecture summary
        let architecture_summary = build_architecture_summary(
            &build_systems,
            &languages,
            &directory_roles,
            &file_roles,
            &entry_points,
            &file_tree,
        );

        // 9. Compute totals
        let total_files = file_tree.total_file_count;
        let total_dirs = count_directories(&file_tree);
        let total_lines = file_tree.total_lines;

        let project_name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let analysis = ProjectAnalysis {
            project_root: project_root.to_string_lossy().to_string(),
            project_name,
            file_tree,
            build_systems,
            languages,
            directory_roles,
            file_roles,
            entry_points,
            architecture_summary,
            total_files,
            total_dirs,
            total_lines,
        };

        info!(
            "ProjectAnalyzer: analysis complete in {:?} — {} files, {} dirs, {} build systems",
            start.elapsed(),
            total_files,
            total_dirs,
            analysis.build_systems.len(),
        );

        Ok(analysis)
    }

    /// Generate a concise text summary of the project.
    async fn summarize(&self, project_root: &Path) -> Result<String> {
        let analysis = self.analyze(project_root).await?;
        Ok(format_project_summary(&analysis))
    }
}

// ============================================================================
// Actor Trait Implementation
// ============================================================================

#[async_trait]
impl Actor for ProjectAnalyzerActor {
    type Message = ProjectAnalyzerMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            ProjectAnalyzerMessage::Initialize {
                reply_to,
            } => {
                info!("ProjectAnalyzerActor: initialized");
                let _ = reply_to.send(Ok(()));
            }
            ProjectAnalyzerMessage::Analyze {
                project_root,
                reply_to,
            } => {
                let result = self.analyze(&project_root).await;
                let _ = reply_to.send(result);
            }
            ProjectAnalyzerMessage::Summarize {
                project_root,
                reply_to,
            } => {
                let result = self.summarize(&project_root).await;
                let _ = reply_to.send(result);
            }
        }
    }
}

// ============================================================================
// Tree Traversal Helpers
// ============================================================================

/// Recursively collect language statistics from the file tree.
fn collect_languages_from_tree(
    dir: &DirectoryNode,
    map: &mut std::collections::HashMap<String, (usize, usize)>,
) {
    for file in &dir.files {
        let entry = map.entry(file.language.clone()).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += file.lines_estimated;
    }
    for subdir in &dir.directories {
        collect_languages_from_tree(subdir, map);
    }
}

/// Recursively collect directory role statistics.
fn collect_directory_roles(
    dir: &DirectoryNode,
    map: &mut std::collections::HashMap<String, usize>,
) {
    *map.entry(dir.role.clone()).or_insert(0) += 1;
    for subdir in &dir.directories {
        collect_directory_roles(subdir, map);
    }
}

/// Recursively collect file role statistics.
fn collect_file_roles(
    dir: &DirectoryNode,
    map: &mut std::collections::HashMap<String, usize>,
) {
    for file in &dir.files {
        *map.entry(file.role.clone()).or_insert(0) += 1;
    }
    for subdir in &dir.directories {
        collect_file_roles(subdir, map);
    }
}

/// Recursively find entry point files.
fn find_entry_points(dir: &DirectoryNode) -> Vec<String> {
    let mut entries = Vec::new();
    for file in &dir.files {
        if file.role == "entry_point" {
            entries.push(file.path.clone());
        }
    }
    for subdir in &dir.directories {
        entries.extend(find_entry_points(subdir));
    }
    entries
}

/// Count total directories in the tree (recursive).
fn count_directories(dir: &DirectoryNode) -> usize {
    let mut count = 1; // this directory
    for subdir in &dir.directories {
        count += count_directories(subdir);
    }
    count
}

// ============================================================================
// Summary Builders
// ============================================================================

/// Build a human-readable architecture summary.
fn build_architecture_summary(
    build_systems: &[BuildMetadata],
    languages: &[LanguageBreakdown],
    directory_roles: &[RoleBreakdown],
    file_roles: &[RoleBreakdown],
    entry_points: &[String],
    file_tree: &DirectoryNode,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Project type
    if let Some(bs) = build_systems.first() {
        parts.push(format!(
            "Build system: {} ({})",
            bs.build_system, bs.project_type
        ));
    }

    // Languages
    if !languages.is_empty() {
        let lang_str: Vec<String> = languages
            .iter()
            .map(|l| format!("{} ({} files)", l.language, l.file_count))
            .collect();
        parts.push(format!("Languages: {}", lang_str.join(", ")));
    }

    // Directory structure
    if !directory_roles.is_empty() {
        let dir_str: Vec<String> = directory_roles
            .iter()
            .map(|r| format!("{}: {}", r.role, r.count))
            .collect();
        parts.push(format!("Directory structure: {}", dir_str.join(", ")));
    }

    // File roles
    if !file_roles.is_empty() {
        let file_str: Vec<String> = file_roles
            .iter()
            .map(|r| format!("{}: {}", r.role, r.count))
            .collect();
        parts.push(format!("File types: {}", file_str.join(", ")));
    }

    // Entry points
    if !entry_points.is_empty() {
        parts.push(format!("Entry points: {}", entry_points.join(", ")));
    }

    // Totals
    parts.push(format!(
        "Total: {} files, {} directories, ~{} lines of code",
        file_tree.total_file_count,
        count_directories(file_tree),
        file_tree.total_lines,
    ));

    parts.join("\n")
}

/// Format the full project analysis as a human-readable summary string.
fn format_project_summary(analysis: &ProjectAnalysis) -> String {
    let mut output = String::new();

    output.push_str(&format!("Project: {}\n", analysis.project_name));
    output.push_str(&format!("Root: {}\n", analysis.project_root));
    output.push_str(&format!("Files: {}, Dirs: {}, Lines: ~{}\n\n",
        analysis.total_files, analysis.total_dirs, analysis.total_lines));

    // Build systems
    if !analysis.build_systems.is_empty() {
        output.push_str("=== Build Systems ===\n");
        for bs in &analysis.build_systems {
            output.push_str(&format!("  {} ({})\n", bs.build_system, bs.project_type));
            if let Some(ref name) = bs.project_name {
                output.push_str(&format!("    Name: {}\n", name));
            }
            if let Some(ref ver) = bs.version {
                output.push_str(&format!("    Version: {}\n", ver));
            }
            if !bs.scripts.is_empty() {
                output.push_str("    Scripts:\n");
                for script in &bs.scripts {
                    output.push_str(&format!("      {}: {}\n", script.name, script.command));
                }
            }
        }
        output.push('\n');
    }

    // Languages
    if !analysis.languages.is_empty() {
        output.push_str("=== Languages ===\n");
        for lang in &analysis.languages {
            output.push_str(&format!("  {}: {} files, ~{} lines\n",
                lang.language, lang.file_count, lang.line_estimate));
        }
        output.push('\n');
    }

    // Directory roles
    if !analysis.directory_roles.is_empty() {
        output.push_str("=== Directory Structure ===\n");
        for role in &analysis.directory_roles {
            output.push_str(&format!("  {}: {} dirs\n", role.role, role.count));
        }
        output.push('\n');
    }

    // Entry points
    if !analysis.entry_points.is_empty() {
        output.push_str("=== Entry Points ===\n");
        for ep in &analysis.entry_points {
            output.push_str(&format!("  {}\n", ep));
        }
        output.push('\n');
    }

    // Architecture summary
    output.push_str("=== Architecture Summary ===\n");
    output.push_str(&analysis.architecture_summary);
    output.push('\n');

    output
}
