// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Project Analyzer — standalone library for analysing project directory structure.
//!
//! This module provides a comprehensive project analysis pipeline:
//!
//! 1. **Scanner** — Walk the filesystem, respecting `.gitignore`, collecting `FileInfo`.
//! 2. **Build parsers** — Parse build config files (Cargo.toml, package.json, etc.)
//!    into normalized `BuildMetadata`.
//! 3. **Tree builder** — Assemble the flat file list into a hierarchical
//!    `DirectoryNode` tree with language detection, role classification, and
//!    line estimation.
//! 4. **Rust analyzer** — Run `cargo metadata` for rich Rust project metadata.
//!
//! The top-level entry point is [`analyze_project`], which runs the full pipeline
//! and returns a [`ProjectFileTree`].

pub mod models;
pub mod scanner;
pub mod tree_builder;
pub mod rust_analyzer;
pub mod build_parsers;

use std::path::Path;

pub use models::*;
pub use scanner::*;
pub use tree_builder::*;

/// Run the full project analysis pipeline on a directory.
///
/// This is the main entry point. It:
/// 1. Scans the directory for all files
/// 2. Discovers build config files
/// 3. Parses each build config into normalized metadata
/// 4. Builds the hierarchical file tree
/// 5. Detects languages
/// 6. Returns a complete `ProjectFileTree`
pub fn analyze_project(root: &Path, no_ignore: bool) -> ProjectFileTree {
    // Stage 1: Scan the filesystem
    let files = scanner::scan_directory(root, no_ignore);
    let non_dir_files: Vec<&FileInfo> = files.iter().filter(|f| !f.is_dir).collect();

    // Stage 2: Discover build config files
    let build_files = scanner::discover_build_files(root, no_ignore);

    // Stage 3: Parse build configs
    let build_metadata: Vec<BuildMetadata> = build_files
        .iter()
        .filter_map(|(build_file, _)| {
            build_parsers::parse_build_file(root, build_file, &files)
        })
        .collect();

    // Merge build metadata from all build files
    let build = merge_build_metadata(&build_metadata);

    // Stage 4: Build the file tree
    let root_node = tree_builder::build_file_tree(root, no_ignore);

    // Stage 5: Detect languages
    let languages = tree_builder::detect_languages(&non_dir_files);

    ProjectFileTree {
        root: root_node,
        build,
        languages,
    }
}

/// Merge multiple BuildMetadata entries into a single summary.
///
/// When there are multiple build files (e.g. Cargo.toml + package.json),
/// we merge them into one coherent picture. The first entry's project type
/// and build system take precedence, but we collect all scripts, features,
/// targets, and config files.
fn merge_build_metadata(metadata_list: &[BuildMetadata]) -> BuildMetadata {
    if metadata_list.is_empty() {
        return BuildMetadata {
            project_name: None,
            version: None,
            project_type: "unknown".to_string(),
            build_system: "unknown".to_string(),
            is_workspace: false,
            workspace_members: vec![],
            scripts: vec![],
            features: vec![],
            targets: vec![],
            config_files: vec![],
            raw: None,
        };
    }

    let primary = &metadata_list[0];
    let mut merged = primary.clone();

    for secondary in &metadata_list[1..] {
        // Merge scripts (avoid duplicates)
        for script in &secondary.scripts {
            if !merged.scripts.iter().any(|s| s.name == script.name) {
                merged.scripts.push(script.clone());
            }
        }

        // Merge features (avoid duplicates)
        for feature in &secondary.features {
            if !merged.features.iter().any(|f| f.name == feature.name) {
                merged.features.push(feature.clone());
            }
        }

        // Merge targets (avoid duplicates)
        for target in &secondary.targets {
            if !merged.targets.iter().any(|t| t.name == target.name) {
                merged.targets.push(target.clone());
            }
        }

        // Merge config files
        for config in &secondary.config_files {
            if !merged.config_files.contains(config) {
                merged.config_files.push(config.clone());
            }
        }

        // Merge workspace members
        for member in &secondary.workspace_members {
            if !merged.workspace_members.iter().any(|m| m.name == member.name) {
                merged.workspace_members.push(member.clone());
            }
        }

        // If primary has no project name but secondary does, use it
        if merged.project_name.is_none() {
            merged.project_name = secondary.project_name.clone();
        }

        // If primary has no version but secondary does, use it
        if merged.version.is_none() {
            merged.version = secondary.version.clone();
        }
    }

    merged
}
