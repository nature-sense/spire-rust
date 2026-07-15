// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Build system parsers — convert native build config files into normalized
//! [`BuildMetadata`].
//!
//! Supported build systems:
//! - **Cargo** (Rust) — via `cargo_metadata` or simple TOML parsing
//! - **npm/pnpm/Yarn** (Node.js) — via `package.json` and `pnpm-workspace.yaml`
//! - **Meson** — via `meson.build` line-based parsing
//! - **Python** — via `pyproject.toml`, `setup.py`, `setup.cfg`
//! - **Go** — via `go.mod`
//! - **Gradle** — via `build.gradle` / `build.gradle.kts`
//! - **Maven** — via `pom.xml`
//! - **CMake** — via `CMakeLists.txt`
//! - **Make** — via `Makefile`

pub mod cargo;
pub mod node;
pub mod meson;
pub mod python;
pub mod go;
pub mod gradle;
pub mod maven;
pub mod cmake;
pub mod make;

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a build file and return normalized BuildMetadata.
///
/// This is the main entry point for build system parsing. It detects the
/// build system from the filename and delegates to the appropriate parser.
///
/// The `build_file` parameter is a path relative to `project_root` (e.g.
/// `"Cargo.toml"` or `"rust/Cargo.toml"`). We extract the filename to
/// determine the build system, then compute the correct base directory
/// so that parsers can find their config files in the right location.
pub fn parse_build_file(
    project_root: &Path,
    build_file: &str,
    files: &[crate::analyzer::models::FileInfo],
) -> Option<BuildMetadata> {
    let path = std::path::Path::new(build_file);
    let filename = path.file_name()?.to_str()?;
    // Compute the directory containing the build file, relative to project_root.
    // For "Cargo.toml" this is ".", for "rust/Cargo.toml" this is "rust".
    let parent_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let parser_root = project_root.join(parent_dir);

    match filename {
        "Cargo.toml" => cargo::parse_cargo(&parser_root),
        "package.json" => node::parse_package_json(&parser_root),
        "pnpm-workspace.yaml" => node::parse_pnpm_workspace(&parser_root),
        "meson.build" => meson::parse_meson_build(&parser_root),
        "pyproject.toml" | "setup.py" | "setup.cfg" => python::parse_python(&parser_root, filename),
        "go.mod" => go::parse_go_mod(&parser_root),
        "build.gradle" | "build.gradle.kts" => gradle::parse_gradle(&parser_root, filename),
        "pom.xml" => maven::parse_pom(&parser_root),
        "CMakeLists.txt" => cmake::parse_cmake(&parser_root),
        "Makefile" => make::parse_makefile(&parser_root),
        _ => None,
    }
}

/// Detect the project type from build files present.
pub fn detect_project_type_from_files(files: &[crate::analyzer::models::FileInfo]) -> (String, f64) {
    let configs: std::collections::HashSet<&str> = files
        .iter()
        .map(|f| f.relative_path.as_str())
        .collect();

    // Rust
    if configs.contains("Cargo.toml") || configs.contains("rust/Cargo.toml") {
        if has_rust_workspace(files) {
            return ("rust_workspace".to_string(), 0.95);
        }
        let cargo_count = files.iter().filter(|f| f.relative_path.ends_with("Cargo.toml")).count();
        if cargo_count > 1 {
            return ("rust_workspace".to_string(), 0.90);
        }
        return ("rust_crate".to_string(), 0.95);
    }

    // Node/TypeScript
    if configs.contains("package.json") {
        if configs.contains("pnpm-workspace.yaml") {
            return ("pnpm_workspace".to_string(), 0.95);
        }
        if has_vscode_extension(files) {
            return ("vscode_extension".to_string(), 0.90);
        }
        return ("node_package".to_string(), 0.95);
    }

    // Meson
    if configs.contains("meson.build") {
        return ("meson_project".to_string(), 0.95);
    }

    // Python
    if configs.contains("pyproject.toml") || configs.contains("setup.py") || configs.contains("setup.cfg") {
        return ("python_project".to_string(), 0.90);
    }

    // Go
    if configs.contains("go.mod") {
        return ("go_module".to_string(), 0.95);
    }

    // Gradle
    if configs.contains("build.gradle") || configs.contains("build.gradle.kts") {
        return ("gradle_project".to_string(), 0.90);
    }

    // Maven
    if configs.contains("pom.xml") {
        return ("maven_project".to_string(), 0.95);
    }

    // CMake
    if configs.contains("CMakeLists.txt") {
        return ("cmake_project".to_string(), 0.90);
    }

    // Make
    if configs.contains("Makefile") {
        return ("make_project".to_string(), 0.70);
    }

    ("unknown".to_string(), 0.0)
}

fn has_rust_workspace(files: &[crate::analyzer::models::FileInfo]) -> bool {
    if let Some(f) = files.iter().find(|f| f.relative_path == "Cargo.toml") {
        if let Ok(content) = std::fs::read_to_string(&f.path) {
            if content.contains("[workspace]") {
                return true;
            }
        }
    }
    if let Some(f) = files.iter().find(|f| f.relative_path == "rust/Cargo.toml") {
        if let Ok(content) = std::fs::read_to_string(&f.path) {
            return content.contains("[workspace]");
        }
    }
    false
}

fn has_vscode_extension(files: &[crate::analyzer::models::FileInfo]) -> bool {
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
