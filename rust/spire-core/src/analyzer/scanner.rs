// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Filesystem scanner — walks a directory tree and collects file metadata.
//!
//! Two scanning modes:
//! - **Standard** (default): Uses the `ignore` crate to respect `.gitignore`.
//! - **No-ignore**: Uses `walkdir` for a simple recursive listing, skipping
//!   only hidden directories and known non-project directories.

use std::collections::HashMap;
use std::path::Path;

use ignore::WalkBuilder;
use walkdir::WalkDir;

use crate::analyzer::models::FileInfo;

/// Known build config file names used for Stage 1 discovery.
/// These are the primary build manifests that define a project root.
pub const BUILD_CONFIG_FILES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pnpm-workspace.yaml",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "go.mod",
    "build.gradle",
    "build.gradle.kts",
    "pom.xml",
    "CMakeLists.txt",
    "Makefile",
    "Gemfile",
    "Package.swift",
];

/// Directories that should always be skipped during discovery (e.g. dependencies, build output).
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".pnpm",
    "target",
    "dist",
    "build",
    "out",
    ".git",
    ".svn",
    ".hg",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".eggs",
    "eggs",
];

/// Scan a directory and return all files, respecting .gitignore.
pub fn scan_directory(root: &Path, no_ignore: bool) -> Vec<FileInfo> {
    let mut files = Vec::new();

    if no_ignore {
        // Use walkdir for simple recursive listing without .gitignore
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_hidden(e.file_name()))
        {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    let relative = path
                        .strip_prefix(root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();

                    // Skip common non-project directories
                    if should_skip(&relative) {
                        continue;
                    }

                    let ext = path
                        .extension()
                        .map(|e| format!(".{}", e.to_string_lossy()))
                        .unwrap_or_default();

                    let ft = entry.file_type();
                    files.push(FileInfo {
                        path: path.to_string_lossy().to_string(),
                        relative_path: relative,
                        extension: ext,
                        size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                        is_dir: ft.is_dir(),
                        is_symlink: ft.is_symlink(),
                    });
                }
                Err(_) => continue,
            }
        }
    } else {
        // Use the `ignore` crate which respects .gitignore
        let walker = WalkBuilder::new(root)
            .standard_filters(true)
            .follow_links(false)
            .build();

        for result in walker {
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    let relative = path
                        .strip_prefix(root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();

                    if should_skip(&relative) {
                        continue;
                    }

                    let ext = path
                        .extension()
                        .map(|e| format!(".{}", e.to_string_lossy()))
                        .unwrap_or_default();

                    let meta = entry.metadata().ok();
                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or_else(|| {
                        meta.as_ref().map(|m| m.is_dir()).unwrap_or(false)
                    });
                    let is_symlink = entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false);
                    files.push(FileInfo {
                        path: path.to_string_lossy().to_string(),
                        relative_path: relative,
                        extension: ext,
                        size: meta.map(|m| m.len()).unwrap_or(0),
                        is_dir,
                        is_symlink,
                    });
                }
                Err(_) => continue,
            }
        }
    }

    files
}

/// Stage 1: Walk the directory tree looking for build config files.
/// Returns a list of (relative_path_to_build_file, parent_dir_relative) pairs.
/// The parent_dir is the directory containing the build file, relative to root.
pub fn discover_build_files(root: &Path, _no_ignore: bool) -> Vec<(String, String)> {
    let mut results = Vec::new();

    // Use a filtering walker that skips known non-project directories
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden dirs
            if name.starts_with('.') {
                return false;
            }
            // Skip known non-project dirs
            if e.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref()) {
                return false;
            }
            true
        });

    for entry in walker {
        match entry {
            Ok(entry) => {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();

                // Check if this filename matches a known build config
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if BUILD_CONFIG_FILES.contains(&filename) {
                    // Get the parent directory
                    let parent = Path::new(&relative)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| ".".to_string());
                    results.push((relative, parent));
                }
            }
            Err(_) => continue,
        }
    }

    results
}

/// Group files by their top-level directory.
/// Root-level files (no '/' in path) are grouped under ".".
pub fn group_by_top_dir(files: &[FileInfo]) -> HashMap<String, Vec<&FileInfo>> {
    let mut groups: HashMap<String, Vec<&FileInfo>> = HashMap::new();

    for file in files {
        let top = if file.relative_path.contains('/') {
            file.relative_path
                .split('/')
                .next()
                .unwrap_or(".")
                .to_string()
        } else {
            // Root-level files (e.g. README.md, Cargo.toml) → root group
            ".".to_string()
        };

        groups.entry(top).or_default().push(file);
    }

    groups
}

/// Check if a file path should be skipped (common non-project directories).
fn should_skip(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    // Skip hidden directories/files at any level
    parts.iter().any(|p| is_hidden(p.as_ref()))
}

/// Check if a filename is hidden (starts with `.`).
fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}
