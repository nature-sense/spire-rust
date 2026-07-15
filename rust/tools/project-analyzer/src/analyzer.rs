use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::models::*;
use crate::rust_analyzer;
use crate::scanner;

/// Maximum recursion depth for sub-project analysis.
const MAX_DEPTH: usize = 5;

// ═══════════════════════════════════════════════════════════════════════════════
// Three-stage pipeline
// ═══════════════════════════════════════════════════════════════════════════════

/// Analyze a project directory using the three-stage approach:
///
/// 1. **Discovery** — Walk the tree looking for build config files (Cargo.toml,
///    package.json, etc.) to identify all build projects.
/// 2. **Per-project analysis** — For each discovered project, parse its build
///    file, detect languages, entry points, key files, and sub-projects.
/// 3. **Remaining context** — Classify directories not claimed by any build
///    project (docs, scripts, config, etc.).
pub fn analyze_project(root: &Path, no_ignore: bool) -> ProjectAnalysis {
    // ── Stage 1: Discover build files ──────────────────────────────────────
    let build_file_hits = scanner::discover_build_files(root, no_ignore);

    // Deduplicate by parent directory (a dir may have multiple build files)
    let mut project_roots: Vec<(String, String)> = Vec::new();
    let mut seen_dirs: HashSet<String> = HashSet::new();
    for (build_file, parent_dir) in &build_file_hits {
        if seen_dirs.insert(parent_dir.clone()) {
            project_roots.push((build_file.clone(), parent_dir.clone()));
        }
    }

    // ── Stage 2: Analyze each discovered project ───────────────────────────
    let mut visited = HashSet::new();
    let mut build_projects: Vec<BuildProject> = Vec::new();
    let mut claimed_dirs: HashSet<String> = HashSet::new();

    // Sort by depth (shallowest first) so parent workspaces are processed first
    project_roots.sort_by(|a, b| a.1.matches('/').count().cmp(&b.1.matches('/').count()));

    for (build_file_rel, parent_dir) in &project_roots {
        let project_root = if parent_dir == "." {
            root.to_path_buf()
        } else {
            root.join(parent_dir)
        };

        if !project_root.is_dir() {
            continue;
        }

        // Mark this directory as claimed
        claimed_dirs.insert(parent_dir.clone());

        // Extract just the filename from the build file path
        let build_file_name = Path::new(build_file_rel)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(build_file_rel);

        let bp = analyze_build_project(
            &project_root,
            root,
            build_file_name,
            parent_dir,
            no_ignore,
            0,
            &mut visited,
        );
        build_projects.push(bp);
    }

    // ── Stage 2b: Full scan for overall project analysis ───────────────────
    let all_files = scanner::scan_directory(root, no_ignore);
    let non_dir_files: Vec<&FileInfo> = all_files.iter().filter(|f| !f.is_dir).collect();

    let (project_type, confidence) = detect_project_type(&non_dir_files);
    let languages = detect_languages(&non_dir_files);
    let build_tools = detect_build_tools(&non_dir_files);
    let entry_points = find_entry_points(&non_dir_files, &project_type);
    let directory_structure = build_directory_structure(&all_files, root);
    let key_files = find_key_files(&non_dir_files);

    // ── Stage 3: Analyze remaining (unclaimed) directories ─────────────────
    let misc_directories = analyze_misc_directories(root, &all_files, &claimed_dirs);

    // ── Generate summary ───────────────────────────────────────────────────
    let summary = generate_summary(
        &project_type,
        &languages,
        &build_tools,
        &entry_points,
        root,
        &build_projects,
    );

    ProjectAnalysis {
        root: root.to_string_lossy().to_string(),
        project_type,
        confidence,
        languages,
        build_tools,
        entry_points,
        directory_structure,
        key_files,
        summary,
        sub_projects: vec![], // kept for backward compat; build_projects replaces this
        build_projects,
        misc_directories,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stage 2: Per-build-project analysis
// ═══════════════════════════════════════════════════════════════════════════════

/// Analyze a single build project discovered in Stage 1.
fn analyze_build_project(
    project_root: &Path,
    overall_root: &Path,
    build_file: &str,
    parent_dir: &str,
    no_ignore: bool,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
) -> BuildProject {
    // Prevent cycles
    let canonical = project_root.canonicalize().unwrap_or_else(|_| project_root.to_path_buf());
    if !visited.insert(canonical.clone()) {
        return BuildProject {
            root: parent_dir.to_string(),
            build_file: build_file.to_string(),
            project_type: "already_analyzed".to_string(),
            confidence: 0.0,
            name: None,
            version: None,
            is_workspace: None,
            workspace_members: vec![],
            languages: vec![],
            entry_points: vec![],
            key_files: vec![],
            sub_projects: vec![],
            cargo_info: None,
        };
    }

    // Scan files in this project's subtree
    let files = scanner::scan_directory(project_root, no_ignore);
    let non_dir_files: Vec<&FileInfo> = files.iter().filter(|f| !f.is_dir).collect();

    // Detect project type
    let (project_type, confidence) = detect_project_type(&non_dir_files);

    // Extract name and version from build file
    let (name, version) = extract_project_metadata(project_root, build_file);

    // Detect workspace members
    let (is_workspace, workspace_members) = detect_workspace_info(project_root, &non_dir_files);

    // Detect languages
    let languages = detect_languages(&non_dir_files);

    // Find entry points
    let entry_points = find_entry_points(&non_dir_files, &project_type);

    // Find key files
    let key_files = find_key_files(&non_dir_files);

    // Recursively analyze sub-projects (workspace members)
    let sub_projects = if depth < MAX_DEPTH && is_workspace.unwrap_or(false) {
        analyze_sub_build_projects(project_root, &non_dir_files, overall_root, no_ignore, depth + 1, visited)
    } else {
        vec![]
    };

    // Run cargo_metadata for Rust projects to get rich metadata
    let cargo_info = if project_type.contains("rust") && build_file == "Cargo.toml" {
        rust_analyzer::analyze_rust_project(project_root)
    } else {
        None
    };

    BuildProject {
        root: parent_dir.to_string(),
        build_file: build_file.to_string(),
        project_type,
        confidence,
        name,
        version,
        is_workspace,
        workspace_members,
        languages,
        entry_points,
        key_files,
        sub_projects,
        cargo_info,
    }
}

/// Recursively analyze workspace member sub-projects.
fn analyze_sub_build_projects(
    root: &Path,
    files: &[&FileInfo],
    overall_root: &Path,
    no_ignore: bool,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
) -> Vec<BuildProject> {
    let mut sub_projects = Vec::new();

    // Try Cargo workspace members
    if let Some(members) = detect_cargo_workspace_members(root, files) {
        for member_path in members {
            if member_path.is_dir() {
                let rel = member_path
                    .strip_prefix(overall_root)
                    .unwrap_or(&member_path)
                    .to_string_lossy()
                    .to_string();
                // Use just the filename for the build_file parameter
                let build_file_name = "Cargo.toml";
                let bp = analyze_build_project(
                    &member_path,
                    overall_root,
                    build_file_name,
                    &rel,
                    no_ignore,
                    depth,
                    visited,
                );
                sub_projects.push(bp);
            }
        }
        return sub_projects;
    }

    // Try pnpm/yarn workspace members
    if let Some(members) = detect_pnpm_workspace_members(root, files) {
        for member_path in members {
            if member_path.is_dir() {
                let rel = member_path
                    .strip_prefix(overall_root)
                    .unwrap_or(&member_path)
                    .to_string_lossy()
                    .to_string();
                let build_file_name = "package.json";
                let bp = analyze_build_project(
                    &member_path,
                    overall_root,
                    build_file_name,
                    &rel,
                    no_ignore,
                    depth,
                    visited,
                );
                sub_projects.push(bp);
            }
        }
        return sub_projects;
    }

    // Try npm/yarn workspaces from package.json
    if let Some(members) = detect_npm_workspace_members(root, files) {
        for member_path in members {
            if member_path.is_dir() {
                let rel = member_path
                    .strip_prefix(overall_root)
                    .unwrap_or(&member_path)
                    .to_string_lossy()
                    .to_string();
                let build_file_name = "package.json";
                let bp = analyze_build_project(
                    &member_path,
                    overall_root,
                    build_file_name,
                    &rel,
                    no_ignore,
                    depth,
                    visited,
                );
                sub_projects.push(bp);
            }
        }
    }

    sub_projects
}

/// Extract project name and version from a build config file.
fn extract_project_metadata(project_root: &Path, build_file: &str) -> (Option<String>, Option<String>) {
    let build_path = project_root.join(build_file);
    let content = match std::fs::read_to_string(&build_path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };

    match Path::new(build_file).file_name().and_then(|n| n.to_str()) {
        Some("Cargo.toml") => {
            // Parse TOML for [package] name and version
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
            (name, version)
        }
        Some("package.json") => {
            let json: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return (None, None),
            };
            let name = json.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let version = json.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());
            (name, version)
        }
        Some("pyproject.toml") => {
            let mut name = None;
            let mut version = None;
            let mut in_project = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == "[project]" {
                    in_project = true;
                    continue;
                }
                if in_project {
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
            (name, version)
        }
        _ => (None, None),
    }
}

/// Detect workspace info from build config files.
fn detect_workspace_info(root: &Path, files: &[&FileInfo]) -> (Option<bool>, Vec<String>) {
    // Check Cargo workspace
    if let Some(members) = detect_cargo_workspace_members(root, files) {
        let member_names: Vec<String> = members
            .iter()
            .filter_map(|m| m.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        return (Some(true), member_names);
    }

    // Check pnpm workspace
    if let Some(members) = detect_pnpm_workspace_members(root, files) {
        let member_names: Vec<String> = members
            .iter()
            .filter_map(|m| m.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        return (Some(true), member_names);
    }

    // Check npm/yarn workspace
    if let Some(members) = detect_npm_workspace_members(root, files) {
        let member_names: Vec<String> = members
            .iter()
            .filter_map(|m| m.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        return (Some(true), member_names);
    }

    (None, vec![])
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stage 3: Miscellaneous directory analysis
// ═══════════════════════════════════════════════════════════════════════════════

/// Analyze directories not claimed by any build project.
fn analyze_misc_directories(
    _root: &Path,
    all_files: &[FileInfo],
    claimed_dirs: &HashSet<String>,
) -> Vec<MiscDirectory> {
    let groups = scanner::group_by_top_dir(all_files);
    let mut misc = Vec::new();

    for (dir_name, dir_files) in &groups {
        // Skip root and claimed directories
        if dir_name == "." || claimed_dirs.contains(dir_name) {
            continue;
        }

        // Only include actual directories (not single files masquerading as dirs)
        let has_subdir = dir_files.iter().any(|f| f.is_dir);
        let file_count = dir_files.iter().filter(|f| !f.is_dir).count();
        if !has_subdir && file_count <= 1 {
            continue;
        }

        let role = classify_misc_directory(dir_name);

        // Detect languages in this directory
        let non_dir: Vec<&FileInfo> = dir_files.iter().filter(|f| !f.is_dir).copied().collect();
        let languages = detect_languages(&non_dir);

        misc.push(MiscDirectory {
            path: dir_name.clone(),
            role,
            file_count,
            languages,
        });
    }

    // Sort: documentation first, then scripts, then others
    misc.sort_by(|a, b| {
        let a_priority = misc_role_priority(&a.role);
        let b_priority = misc_role_priority(&b.role);
        a_priority.cmp(&b_priority).then(a.path.cmp(&b.path))
    });

    misc
}

/// Classify a miscellaneous directory by its name.
fn classify_misc_directory(name: &str) -> String {
    match name {
        "doc" | "docs" | "documentation" => "documentation".to_string(),
        "scripts" | "bin" | "tool" | "tools" => "build_scripts".to_string(),
        "config" | "cfg" | "configuration" | "settings" => "configuration".to_string(),
        "docker" | "container" => "docker".to_string(),
        "ci" | ".github" | ".gitlab" | ".circleci" => "ci_cd".to_string(),
        "assets" | "static" | "public" | "images" | "img" | "media" => "static_assets".to_string(),
        "dist" | "build" | "target" | "out" | "output" => "build_output".to_string(),
        "node_modules" | ".pnpm" | "vendor" | "third_party" | "third-party" => "dependencies".to_string(),
        "examples" | "example" | "samples" | "sample" => "examples".to_string(),
        "tests" | "test" | "spec" | "specs" => "tests".to_string(),
        "benchmarks" | "bench" => "benchmarks".to_string(),
        "data" | "dataset" | "datasets" => "data".to_string(),
        "migrations" | "migrate" => "migrations".to_string(),
        "patches" | "patch" => "patches".to_string(),
        "plugins" | "plugin" | "extensions" | "extension" => "plugins".to_string(),
        "templates" | "template" => "templates".to_string(),
        "i18n" | "locale" | "locales" | "translations" => "localization".to_string(),
        "proto" | "protobuf" | "protos" => "protobuf".to_string(),
        "grafana" | "prometheus" | "monitoring" => "monitoring".to_string(),
        "hack" | "dev" | "development" => "development".to_string(),
        _ => "directory".to_string(),
    }
}

/// Priority for sorting misc directories (lower = shown first).
fn misc_role_priority(role: &str) -> usize {
    match role {
        "documentation" => 0,
        "build_scripts" => 1,
        "configuration" => 2,
        "ci_cd" => 3,
        "docker" => 4,
        "examples" => 5,
        "tests" => 6,
        "static_assets" => 7,
        "data" => 8,
        "templates" => 9,
        "localization" => 10,
        "protobuf" => 11,
        "migrations" => 12,
        "plugins" => 13,
        "benchmarks" => 14,
        "monitoring" => 15,
        "development" => 16,
        "patches" => 17,
        "dependencies" => 18,
        "build_output" => 19,
        _ => 99,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace member detection (shared between old and new code)
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect Cargo workspace members from [workspace.members].
/// Checks both root Cargo.toml and rust/Cargo.toml.
fn detect_cargo_workspace_members(root: &Path, files: &[&FileInfo]) -> Option<Vec<PathBuf>> {
    // Try root Cargo.toml first, then rust/Cargo.toml
    let cargo_file = files.iter()
        .find(|f| f.relative_path == "Cargo.toml")
        .or_else(|| files.iter().find(|f| f.relative_path == "rust/Cargo.toml"))?;
    let content = std::fs::read_to_string(&cargo_file.path).ok()?;
    if !content.contains("[workspace]") {
        return None;
    }

    // Parse members from the TOML content (simple line-by-line parsing)
    let mut in_members = false;
    let mut in_members_array = false;
    let mut members = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        
        // Detect start of [workspace] section
        if trimmed == "[workspace]" || trimmed.starts_with("[workspace.") {
            in_members = true;
            continue;
        }
        
        if !in_members {
            continue;
        }

        // Stop at the next section header
        if trimmed.starts_with('[') && !trimmed.starts_with("[workspace") {
            break;
        }

        // Check for "members = [" on this line (inline or start of multi-line)
        if let Some(idx) = trimmed.find("members = [") {
            let after_bracket = &trimmed[idx + "members = [".len()..];
            // Check if the array closes on the same line
            if let Some(close) = after_bracket.find(']') {
                // Inline array: members = ["spire-core", "mcp/mcp-git"]
                let items_str = &after_bracket[..close];
                for item in items_str.split(',') {
                    let cleaned = item.trim().trim_matches('"').trim_matches('\'');
                    if !cleaned.is_empty() {
                        members.push(root.join(cleaned));
                    }
                }
            } else {
                // Multi-line array: members = [\n    "spire-core",\n    ...
                in_members_array = true;
                // Also parse any items on this line after the bracket
                if !after_bracket.trim().is_empty() {
                    for item in after_bracket.split(',') {
                        let cleaned = item.trim().trim_matches('"').trim_matches('\'');
                        if !cleaned.is_empty() {
                            members.push(root.join(cleaned));
                        }
                    }
                }
            }
            continue;
        }

        // If we're inside a multi-line members array
        if in_members_array {
            if trimmed == "]" {
                in_members_array = false;
                continue;
            }
            let cleaned = trimmed.trim_matches(',').trim_matches('"').trim_matches('\'');
            if !cleaned.is_empty() {
                members.push(root.join(cleaned));
            }
        }
    }

    if members.is_empty() {
        None
    } else {
        Some(members)
    }
}

/// Detect pnpm workspace members from pnpm-workspace.yaml.
fn detect_pnpm_workspace_members(root: &Path, files: &[&FileInfo]) -> Option<Vec<PathBuf>> {
    let yaml_file = files.iter().find(|f| f.relative_path == "pnpm-workspace.yaml")?;
    let content = std::fs::read_to_string(&yaml_file.path).ok()?;

    // Simple YAML parsing for packages array
    let mut in_packages = false;
    let mut members = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "packages:" {
            in_packages = true;
            continue;
        }
        if in_packages {
            if trimmed.starts_with('-') {
                let pattern = trimmed.trim_start_matches('-').trim().trim_matches('\'');
                // Resolve glob patterns (simple case: just directories)
                let glob_path = root.join(pattern);
                // If it's a simple path (no glob chars), add it directly
                if !pattern.contains('*') && !pattern.contains('?') {
                    members.push(glob_path);
                } else {
                    // Try to expand glob using glob crate or simple prefix matching
                    if let Some(parent) = glob_path.parent() {
                        if let Ok(entries) = std::fs::read_dir(parent) {
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                if glob_match(pattern, &name) && entry.path().is_dir() {
                                    members.push(entry.path());
                                }
                            }
                        }
                    }
                }
            } else if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            } else {
                break;
            }
        }
    }

    if members.is_empty() {
        None
    } else {
        Some(members)
    }
}

/// Detect npm/yarn workspace members from package.json workspaces field.
fn detect_npm_workspace_members(root: &Path, files: &[&FileInfo]) -> Option<Vec<PathBuf>> {
    let pkg_file = files.iter().find(|f| f.relative_path == "package.json")?;
    let content = std::fs::read_to_string(&pkg_file.path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let workspaces = json.get("workspaces")?;
    let patterns: Vec<&str> = if let Some(arr) = workspaces.as_array() {
        arr.iter().filter_map(|v| v.as_str()).collect()
    } else if let Some(obj) = workspaces.as_object() {
        // Yarn-style: { "packages": ["packages/*"] }
        obj.get("packages")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default()
    } else {
        return None;
    };

    let mut members = Vec::new();
    for pattern in patterns {
        let glob_path = root.join(pattern);
        if !pattern.contains('*') && !pattern.contains('?') {
            if glob_path.is_dir() {
                members.push(glob_path);
            }
        } else {
            // Expand glob
            if let Some(parent) = glob_path.parent() {
                if let Ok(entries) = std::fs::read_dir(parent) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if glob_match(pattern, &name) && entry.path().is_dir() {
                            members.push(entry.path());
                        }
                    }
                }
            }
        }
    }

    if members.is_empty() {
        None
    } else {
        Some(members)
    }
}

/// Simple glob matching (supports * and ?).
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == name;
    }
    // Simple glob: split on * and check each part
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        // No wildcard, just ?
        if pattern.contains('?') {
            if pattern.len() != name.len() {
                return false;
            }
            for (p, n) in pattern.chars().zip(name.chars()) {
                if p != '?' && p != n {
                    return false;
                }
            }
            return true;
        }
        return pattern == name;
    }

    // Check prefix
    if !parts[0].is_empty() && !name.starts_with(parts[0]) {
        return false;
    }
    // Check suffix
    if !parts.last().unwrap_or(&"").is_empty() && !name.ends_with(parts.last().unwrap_or(&"")) {
        return false;
    }
    // Check middle parts
    let mut pos = parts[0].len();
    for part in &parts[1..parts.len() - 1] {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = name[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }
    true
}

// ═══════════════════════════════════════════════════════════════════════════════
// Project type detection
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect the primary project type based on config files present.
fn detect_project_type(files: &[&FileInfo]) -> (String, f64) {
    let configs: HashSet<&str> = files
        .iter()
        .map(|f| f.relative_path.as_str())
        .collect();

    // Check for Rust workspace (Cargo.toml at root or in rust/ subdirectory)
    if configs.contains("Cargo.toml") || configs.contains("rust/Cargo.toml") {
        // Check if it's a workspace by looking for [workspace] in Cargo.toml
        if has_rust_workspace(files) {
            return ("rust_workspace".to_string(), 0.95);
        }
        // Check for multiple Cargo.toml files (workspace members)
        let cargo_count = files.iter().filter(|f| f.relative_path.ends_with("Cargo.toml")).count();
        if cargo_count > 1 {
            return ("rust_workspace".to_string(), 0.90);
        }
        return ("rust_crate".to_string(), 0.95);
    }

    // Check for Node/TypeScript project
    if configs.contains("package.json") {
        if configs.contains("pnpm-workspace.yaml") {
            return ("pnpm_workspace".to_string(), 0.95);
        }
        if configs.contains("lerna.json") {
            return ("lerna_monorepo".to_string(), 0.90);
        }
        if has_yarn_workspace(files) {
            return ("yarn_workspace".to_string(), 0.90);
        }
        // Check if it's a VS Code extension
        if has_vscode_extension(files) {
            return ("vscode_extension".to_string(), 0.90);
        }
        return ("node_package".to_string(), 0.95);
    }

    // Python project
    if configs.contains("pyproject.toml") || configs.contains("setup.py") || configs.contains("setup.cfg") {
        return ("python_project".to_string(), 0.90);
    }

    // Go project
    if configs.contains("go.mod") {
        return ("go_module".to_string(), 0.95);
    }

    // Java/Gradle project
    if configs.contains("build.gradle") || configs.contains("build.gradle.kts") {
        return ("gradle_project".to_string(), 0.90);
    }

    // Java/Maven project
    if configs.contains("pom.xml") {
        return ("maven_project".to_string(), 0.95);
    }

    // CMake project
    if configs.contains("CMakeLists.txt") {
        return ("cmake_project".to_string(), 0.90);
    }

    // Makefile project
    if configs.contains("Makefile") {
        return ("make_project".to_string(), 0.70);
    }

    // Ruby project
    if configs.contains("Gemfile") {
        return ("ruby_project".to_string(), 0.85);
    }

    // C# / .NET project
    if files.iter().any(|f| f.extension == ".csproj" || f.extension == ".sln") {
        return ("dotnet_project".to_string(), 0.90);
    }

    // Swift project
    if configs.contains("Package.swift") {
        return ("swift_package".to_string(), 0.90);
    }

    // Docker-only project
    if configs.contains("Dockerfile") {
        return ("docker_project".to_string(), 0.60);
    }

    // Generic detection based on dominant language
    let dominant_lang = detect_dominant_language(files);
    if !dominant_lang.is_empty() {
        return (format!("{}_project", dominant_lang.to_lowercase()), 0.50);
    }

    ("unknown".to_string(), 0.0)
}

/// Check if Cargo.toml has a [workspace] section.
/// Checks both root Cargo.toml and rust/Cargo.toml.
fn has_rust_workspace(files: &[&FileInfo]) -> bool {
    // Check root Cargo.toml first
    if let Some(f) = files.iter().find(|f| f.relative_path == "Cargo.toml") {
        if let Ok(content) = std::fs::read_to_string(&f.path) {
            if content.contains("[workspace]") {
                return true;
            }
        }
    }
    // Also check rust/Cargo.toml
    if let Some(f) = files.iter().find(|f| f.relative_path == "rust/Cargo.toml") {
        if let Ok(content) = std::fs::read_to_string(&f.path) {
            return content.contains("[workspace]");
        }
    }
    false
}

/// Check if package.json has workspaces field.
fn has_yarn_workspace(files: &[&FileInfo]) -> bool {
    files.iter()
        .find(|f| f.relative_path == "package.json")
        .and_then(|f| std::fs::read_to_string(&f.path).ok())
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .map(|v| v.get("workspaces").is_some())
        .unwrap_or(false)
}

/// Check if it's a VS Code extension (has contributes/activationEvents in package.json).
fn has_vscode_extension(files: &[&FileInfo]) -> bool {
    files.iter()
        .find(|f| f.relative_path == "package.json")
        .and_then(|f| std::fs::read_to_string(&f.path).ok())
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .map(|v| {
            v.get("contributes").is_some()
                || v.get("activationEvents").is_some()
                || v.get("engines").and_then(|e| e.get("vscode")).is_some()
        })
        .unwrap_or(false)
}

/// Detect the dominant language by file count.
fn detect_dominant_language(files: &[&FileInfo]) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for file in files {
        let lang = extension_to_language(&file.extension);
        if lang != "Unknown" {
            *counts.entry(lang.to_string()).or_insert(0) += 1;
        }
    }
    counts.into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(name, _)| name)
        .unwrap_or_default()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Language detection
// ═══════════════════════════════════════════════════════════════════════════════

/// Detect all languages used in the project.
fn detect_languages(files: &[&FileInfo]) -> Vec<LanguageInfo> {
    let mut lang_map: HashMap<String, LanguageInfo> = HashMap::new();

    for file in files {
        let lang_name = extension_to_language(&file.extension);
        if lang_name == "Unknown" {
            continue;
        }

        let entry = lang_map.entry(lang_name.to_string()).or_insert_with(|| {
            let extensions = extension_to_extensions(lang_name);
            LanguageInfo {
                name: lang_name.to_string(),
                extensions,
                file_count: 0,
                estimated_lines: 0,
            }
        });

        entry.file_count += 1;
        // Estimate lines from file size (rough: ~50 bytes per line average)
        entry.estimated_lines += (file.size / 50) as usize;
    }

    let mut result: Vec<LanguageInfo> = lang_map.into_values().collect();
    result.sort_by(|a, b| b.file_count.cmp(&a.file_count));
    result
}

/// Map a file extension to a language name.
fn extension_to_language(ext: &str) -> &'static str {
    match ext {
        ".rs" => "Rust",
        ".ts" | ".tsx" => "TypeScript",
        ".js" | ".jsx" | ".mjs" | ".cjs" => "JavaScript",
        ".py" => "Python",
        ".go" => "Go",
        ".java" => "Java",
        ".kt" | ".kts" => "Kotlin",
        ".swift" => "Swift",
        ".rb" => "Ruby",
        ".c" | ".h" => "C",
        ".cpp" | ".hpp" | ".cc" | ".cxx" => "C++",
        ".cs" => "C#",
        ".scala" => "Scala",
        ".r" | ".R" => "R",
        ".php" => "PHP",
        ".pl" | ".pm" => "Perl",
        ".lua" => "Lua",
        ".ex" | ".exs" => "Elixir",
        ".erl" | ".hrl" => "Erlang",
        ".hs" => "Haskell",
        ".clj" | ".cljs" | ".cljc" => "Clojure",
        ".zig" => "Zig",
        ".dart" => "Dart",
        ".sh" | ".bash" | ".zsh" => "Shell",
        ".ps1" => "PowerShell",
        ".sql" => "SQL",
        ".html" | ".htm" => "HTML",
        ".css" | ".scss" | ".sass" | ".less" => "CSS",
        ".json" => "JSON",
        ".yaml" | ".yml" => "YAML",
        ".toml" => "TOML",
        ".xml" => "XML",
        ".md" | ".markdown" => "Markdown",
        ".dockerfile" | "Dockerfile" => "Docker",
        _ => "Unknown",
    }
}

/// Map a language name back to its typical extensions.
fn extension_to_extensions(lang: &str) -> Vec<String> {
    match lang {
        "Rust" => vec![".rs".to_string()],
        "TypeScript" => vec![".ts".to_string(), ".tsx".to_string()],
        "JavaScript" => vec![".js".to_string(), ".jsx".to_string(), ".mjs".to_string(), ".cjs".to_string()],
        "Python" => vec![".py".to_string()],
        "Go" => vec![".go".to_string()],
        "Java" => vec![".java".to_string()],
        "Kotlin" => vec![".kt".to_string(), ".kts".to_string()],
        "Swift" => vec![".swift".to_string()],
        "Ruby" => vec![".rb".to_string()],
        "C" => vec![".c".to_string(), ".h".to_string()],
        "C++" => vec![".cpp".to_string(), ".hpp".to_string(), ".cc".to_string(), ".cxx".to_string()],
        "C#" => vec![".cs".to_string()],
        "Shell" => vec![".sh".to_string(), ".bash".to_string(), ".zsh".to_string()],
        "HTML" => vec![".html".to_string(), ".htm".to_string()],
        "CSS" => vec![".css".to_string(), ".scss".to_string(), ".sass".to_string(), ".less".to_string()],
        "JSON" => vec![".json".to_string()],
        "YAML" => vec![".yaml".to_string(), ".yml".to_string()],
        "TOML" => vec![".toml".to_string()],
        "XML" => vec![".xml".to_string()],
        "Markdown" => vec![".md".to_string(), ".markdown".to_string()],
        _ => vec![],
    }
}

/// Detect build tools from config files.
fn detect_build_tools(files: &[&FileInfo]) -> Vec<BuildToolInfo> {
    let configs: HashSet<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();
    let mut tools = Vec::new();

    // Cargo (check both root Cargo.toml and rust/Cargo.toml)
    if configs.contains("Cargo.toml") || configs.contains("rust/Cargo.toml") {
        let is_workspace = has_rust_workspace(files);
        let mut config_files = vec!["Cargo.toml".to_string()];
        if configs.contains("Cargo.lock") {
            config_files.push("Cargo.lock".to_string());
        }
        tools.push(BuildToolInfo {
            name: "Cargo".to_string(),
            config_files,
            is_workspace: Some(is_workspace),
            version: None,
        });
    }

    // npm/pnpm/yarn
    if configs.contains("package.json") {
        if configs.contains("pnpm-lock.yaml") || configs.contains("pnpm-workspace.yaml") {
            let mut config_files = vec!["package.json".to_string()];
            if configs.contains("pnpm-lock.yaml") {
                config_files.push("pnpm-lock.yaml".to_string());
            }
            if configs.contains("pnpm-workspace.yaml") {
                config_files.push("pnpm-workspace.yaml".to_string());
            }
            tools.push(BuildToolInfo {
                name: "pnpm".to_string(),
                config_files,
                is_workspace: Some(configs.contains("pnpm-workspace.yaml")),
                version: None,
            });
        } else if configs.contains("yarn.lock") {
            tools.push(BuildToolInfo {
                name: "Yarn".to_string(),
                config_files: vec!["package.json".to_string(), "yarn.lock".to_string()],
                is_workspace: Some(has_yarn_workspace(files)),
                version: None,
            });
        } else if configs.contains("package-lock.json") {
            tools.push(BuildToolInfo {
                name: "npm".to_string(),
                config_files: vec!["package.json".to_string(), "package-lock.json".to_string()],
                is_workspace: None,
                version: None,
            });
        } else {
            tools.push(BuildToolInfo {
                name: "npm".to_string(),
                config_files: vec!["package.json".to_string()],
                is_workspace: None,
                version: None,
            });
        }
    }

    // Python build tools
    if configs.contains("pyproject.toml") {
        tools.push(BuildToolInfo {
            name: "Python (pyproject.toml)".to_string(),
            config_files: vec!["pyproject.toml".to_string()],
            is_workspace: None,
            version: None,
        });
    }
    if configs.contains("setup.py") {
        tools.push(BuildToolInfo {
            name: "Python setuptools".to_string(),
            config_files: vec!["setup.py".to_string()],
            is_workspace: None,
            version: None,
        });
    }
    if configs.contains("Pipfile") {
        tools.push(BuildToolInfo {
            name: "Pipenv".to_string(),
            config_files: vec!["Pipfile".to_string()],
            is_workspace: None,
            version: None,
        });
    }
    if configs.contains("poetry.lock") {
        tools.push(BuildToolInfo {
            name: "Poetry".to_string(),
            config_files: vec!["poetry.lock".to_string()],
            is_workspace: None,
            version: None,
        });
    }

    // Go
    if configs.contains("go.mod") {
        tools.push(BuildToolInfo {
            name: "Go Modules".to_string(),
            config_files: vec!["go.mod".to_string()],
            is_workspace: None,
            version: None,
        });
    }

    // Gradle
    if configs.contains("build.gradle") || configs.contains("build.gradle.kts") {
        let mut config_files = Vec::new();
        if configs.contains("build.gradle") {
            config_files.push("build.gradle".to_string());
        }
        if configs.contains("build.gradle.kts") {
            config_files.push("build.gradle.kts".to_string());
        }
        if configs.contains("gradlew") {
            config_files.push("gradlew".to_string());
        }
        tools.push(BuildToolInfo {
            name: "Gradle".to_string(),
            config_files,
            is_workspace: None,
            version: None,
        });
    }

    // Maven
    if configs.contains("pom.xml") {
        tools.push(BuildToolInfo {
            name: "Maven".to_string(),
            config_files: vec!["pom.xml".to_string()],
            is_workspace: None,
            version: None,
        });
    }

    // CMake
    if configs.contains("CMakeLists.txt") {
        tools.push(BuildToolInfo {
            name: "CMake".to_string(),
            config_files: vec!["CMakeLists.txt".to_string()],
            is_workspace: None,
            version: None,
        });
    }

    // Make
    if configs.contains("Makefile") {
        tools.push(BuildToolInfo {
            name: "Make".to_string(),
            config_files: vec!["Makefile".to_string()],
            is_workspace: None,
            version: None,
        });
    }

    // esbuild (common in JS/TS projects)
    if files.iter().any(|f| f.relative_path.contains("esbuild")) {
        tools.push(BuildToolInfo {
            name: "esbuild".to_string(),
            config_files: vec![],
            is_workspace: None,
            version: None,
        });
    }

    tools
}

/// Find entry points in the project.
fn find_entry_points(files: &[&FileInfo], project_type: &str) -> Vec<EntryPoint> {
    let mut entries = Vec::new();

    match project_type {
        p if p.contains("rust") => {
            // Rust entry points
            for file in files {
                match file.relative_path.as_str() {
                    "src/main.rs" => entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "binary".to_string(),
                    }),
                    "src/lib.rs" => entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "library".to_string(),
                    }),
                    _ => {}
                }
            }
        }
        p if p.contains("vscode") || p.contains("node") || p.contains("pnpm") || p.contains("yarn") => {
            // Node.js entry points
            for file in files {
                let path = file.relative_path.as_str();
                if path == "src/extension.ts" || path == "src/extension.js" {
                    entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "vscode_extension".to_string(),
                    });
                } else if path == "src/main.ts" || path == "src/main.js" || path == "src/index.ts" || path == "src/index.js" {
                    entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "main_entry".to_string(),
                    });
                } else if path == "bin/cli.js" || path == "bin/cli.ts" || path == "cli.js" || path == "cli.ts" {
                    entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "cli".to_string(),
                    });
                }
            }
            // Also check package.json for "main" or "bin" fields
            if let Some(pkg) = files.iter().find(|f| f.relative_path == "package.json") {
                if let Ok(content) = std::fs::read_to_string(&pkg.path) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(main) = json.get("main").and_then(|v| v.as_str()) {
                            entries.push(EntryPoint {
                                path: main.to_string(),
                                entry_type: "package_main".to_string(),
                            });
                        }
                        if let Some(bin) = json.get("bin") {
                            if let Some(bin_str) = bin.as_str() {
                                entries.push(EntryPoint {
                                    path: bin_str.to_string(),
                                    entry_type: "package_bin".to_string(),
                                });
                            } else if let Some(bin_obj) = bin.as_object() {
                                for (_, val) in bin_obj {
                                    if let Some(path) = val.as_str() {
                                        entries.push(EntryPoint {
                                            path: path.to_string(),
                                            entry_type: "package_bin".to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        p if p.contains("python") => {
            for file in files {
                let path = file.relative_path.as_str();
                if path == "main.py" || path == "app.py" || path == "cli.py" || path.starts_with("src/") && path.ends_with("/__main__.py") {
                    entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "python_entry".to_string(),
                    });
                }
            }
        }
        p if p.contains("go") => {
            for file in files {
                if file.relative_path == "main.go" || file.relative_path.ends_with("/main.go") || file.relative_path.ends_with("/cmd/main.go") {
                    entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "go_entry".to_string(),
                    });
                }
            }
        }
        _ => {
            // Generic: look for common entry point patterns
            for file in files {
                let path = file.relative_path.as_str();
                if path == "main.rs" || path == "main.go" || path == "main.py" || path == "main.ts" || path == "main.js" || path == "index.js" || path == "index.ts" {
                    entries.push(EntryPoint {
                        path: file.relative_path.clone(),
                        entry_type: "main_entry".to_string(),
                    });
                }
            }
        }
    }

    entries
}

/// Build a high-level directory structure summary.
fn build_directory_structure(files: &[FileInfo], _root: &Path) -> HashMap<String, DirEntry> {
    let mut structure: HashMap<String, DirEntry> = HashMap::new();
    let groups = scanner::group_by_top_dir(files);

    for (dir_name, dir_files) in &groups {
        let dir_type = classify_directory(dir_name, dir_files);

        let mut entry = DirEntry {
            dir_type,
            file_count: None,
            sub_projects: None,
            has_src: None,
            has_tests: None,
            has_examples: None,
        };

        // Check for common sub-directory patterns
        let paths: Vec<&str> = dir_files.iter().map(|f| f.relative_path.as_str()).collect();

        entry.has_src = Some(paths.iter().any(|p| p.contains("/src/") || p.starts_with("src/")));
        entry.has_tests = Some(
            paths.iter().any(|p| {
                p.contains("/tests/") || p.contains("/test/") || p.ends_with("_test.rs") || p.ends_with(".test.ts") || p.ends_with(".spec.ts")
            }),
        );
        entry.has_examples = Some(paths.iter().any(|p| p.contains("/examples/")));

        // Count non-directory files
        let file_count = dir_files.iter().filter(|f| !f.is_dir).count();
        entry.file_count = Some(file_count);

        // Detect sub-projects (workspace members)
        if dir_name == "." || dir_name == "mcp" || dir_name == "packages" || dir_name == "crates" {
            let sub_projects: Vec<String> = dir_files
                .iter()
                .filter(|f| f.relative_path.ends_with("Cargo.toml") || f.relative_path.ends_with("package.json"))
                .filter_map(|f| {
                    let parent = Path::new(&f.relative_path).parent()?;
                    let name = parent.file_name()?.to_string_lossy().to_string();
                    if name != "." && !name.starts_with('.') {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();
            if !sub_projects.is_empty() {
                entry.sub_projects = Some(sub_projects);
            }
        }

        structure.insert(dir_name.clone(), entry);
    }

    structure
}

/// Classify a directory by its name and contents.
fn classify_directory(name: &str, files: &[&FileInfo]) -> String {
    match name {
        "." => "root".to_string(),
        "src" => "source_code".to_string(),
        "lib" => "library_code".to_string(),
        "tests" | "test" => "tests".to_string(),
        "examples" => "examples".to_string(),
        "docs" | "doc" => "documentation".to_string(),
        "scripts" | "bin" => "build_scripts".to_string(),
        "config" | "cfg" => "configuration".to_string(),
        "docker" => "docker".to_string(),
        "ci" | ".github" | ".gitlab" => "ci_cd".to_string(),
        "assets" | "static" | "public" => "static_assets".to_string(),
        "dist" | "build" | "target" | "out" => "build_output".to_string(),
        "node_modules" | ".pnpm" => "dependencies".to_string(),
        "mcp" => "mcp_servers".to_string(),
        "spire-core" => "core_crate".to_string(),
        "spire-extension" => "vscode_extension".to_string(),
        _ => {
            // Check if it looks like a workspace member (has Cargo.toml or package.json)
            let has_cargo = files.iter().any(|f| f.relative_path.ends_with("Cargo.toml"));
            let has_package = files.iter().any(|f| f.relative_path.ends_with("package.json"));
            if has_cargo || has_package {
                "workspace_member".to_string()
            } else {
                "directory".to_string()
            }
        }
    }
}

/// Find key files and their roles.
fn find_key_files(files: &[&FileInfo]) -> Vec<KeyFile> {
    let mut key_files = Vec::new();

    for file in files {
        let role = match file.relative_path.as_str() {
            "README.md" | "README" | "README.txt" => Some("documentation"),
            "CHANGELOG.md" | "CHANGELOG" | "HISTORY.md" => Some("changelog"),
            "LICENSE" | "LICENSE.md" | "LICENSE.txt" | "COPYING" => Some("license"),
            "CONTRIBUTING.md" | "CONTRIBUTING" => Some("contributing_guide"),
            "CODE_OF_CONDUCT.md" => Some("code_of_conduct"),
            "TODO.md" | "TODO" | "ROADMAP.md" => Some("todo"),
            "Cargo.toml" => Some("rust_manifest"),
            "package.json" => Some("package_manifest"),
            "pnpm-lock.yaml" => Some("lock_file"),
            "pnpm-workspace.yaml" => Some("js_workspace_config"),
            "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml" => Some("docker"),
            ".gitignore" => Some("git_ignore"),
            ".gitattributes" => Some("git_attributes"),
            ".editorconfig" => Some("editor_config"),
            ".env.example" | ".env.sample" => Some("env_example"),
            "Makefile" => Some("makefile"),
            "tsconfig.json" => Some("tsconfig"),
            "eslintrc.js" | "eslintrc.json" | ".eslintrc" => Some("eslint_config"),
            "prettierrc" | ".prettierrc" | ".prettierrc.js" | ".prettierrc.json" => Some("prettier_config"),
            "jest.config.js" | "jest.config.ts" | "vitest.config.ts" | "vitest.config.js" => Some("test_config"),
            "go.mod" => Some("go_module"),
            "go.sum" => Some("go_checksum"),
            "pyproject.toml" => Some("python_project_config"),
            "setup.py" => Some("python_setup"),
            "build.gradle" | "build.gradle.kts" => Some("gradle_build"),
            "pom.xml" => Some("maven_pom"),
            "Gemfile" => Some("gemfile"),
            "CMakeLists.txt" => Some("cmake_lists"),
            _ => {
                // Check for CI configs
                if file.relative_path.starts_with(".github/") {
                    Some("github_ci")
                } else if file.relative_path.starts_with(".gitlab-ci.yml") || file.relative_path.starts_with(".gitlab-ci.yaml") {
                    Some("gitlab_ci")
                } else if file.relative_path.starts_with(".circleci/") {
                    Some("circle_ci")
                } else {
                    None
                }
            }
        };

        if let Some(role) = role {
            key_files.push(KeyFile {
                path: file.relative_path.clone(),
                role: role.to_string(),
            });
        }
    }

    key_files
}

/// Generate a human-readable summary of the project.
fn generate_summary(
    project_type: &str,
    languages: &[LanguageInfo],
    build_tools: &[BuildToolInfo],
    _entry_points: &[EntryPoint],
    root: &Path,
    build_projects: &[BuildProject],
) -> String {
    let project_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let lang_count = languages.len();
    let top_langs: Vec<&str> = languages
        .iter()
        .take(3)
        .map(|l| l.name.as_str())
        .collect();

    let lang_part = if lang_count == 0 {
        "no source files".to_string()
    } else if lang_count == 1 {
        format!("contains {} only", top_langs[0])
    } else {
        let extra = if lang_count > 3 {
            format!(", and {} more", lang_count - 3)
        } else {
            String::new()
        };
        format!(
            "contains {} ({} files), {} ({} files){extra}",
            top_langs[0],
            languages[0].file_count,
            top_langs[1],
            languages[1].file_count,
        )
    };

    let tool_names: Vec<&str> = build_tools.iter().map(|t| t.name.as_str()).collect();
    let tool_part = if tool_names.is_empty() {
        String::new()
    } else {
        format!(". Uses {}", tool_names.join(", "))
    };

    let project_part = if build_projects.is_empty() {
        String::new()
    } else {
        let project_names: Vec<&str> = build_projects
            .iter()
            .filter_map(|bp| bp.name.as_deref())
            .collect();
        if !project_names.is_empty() {
            format!(". Build projects: {}", project_names.join(", "))
        } else {
            format!(". {} build project(s) detected", build_projects.len())
        }
    };

    format!(
        "{project_name} is a {project_type}. It {lang_part}{tool_part}{project_part}."
    )
}
