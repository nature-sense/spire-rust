use std::path::Path;

use crate::models::*;

/// Parse a CMakeLists.txt and return normalized BuildMetadata.
pub fn parse_cmake(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("CMakeLists.txt");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;
    let mut languages = Vec::new();
    let mut targets = Vec::new();
    let mut dependencies = Vec::new();
    let mut subdirs = Vec::new();
    let mut has_tests = false;
    let mut cmake_minimum = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        // cmake_minimum_required(VERSION 3.16)
        if trimmed.starts_with("cmake_minimum_required(") {
            if let Some(ver) = trimmed.find("VERSION ") {
                let rest = &trimmed[ver + 8..];
                cmake_minimum = Some(rest.trim_end_matches(')').trim().to_string());
            }
            continue;
        }

        // project(MyProject VERSION 1.0 LANGUAGES C CXX)
        if trimmed.starts_with("project(") {
            let content = &trimmed["project(".len()..trimmed.len() - 1];
            let parts: Vec<&str> = content.split_whitespace().collect();
            if let Some(first) = parts.first() {
                name = Some(first.to_string());
            }
            // Parse VERSION and LANGUAGES keywords
            let mut i = 1;
            while i < parts.len() {
                match parts[i] {
                    "VERSION" => {
                        if i + 1 < parts.len() {
                            version = Some(parts[i + 1].to_string());
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    "LANGUAGES" => {
                        i += 1;
                        while i < parts.len() && !parts[i].starts_with("CXX") && !parts[i].starts_with("C") && !parts[i].starts_with("CXX") && !parts[i].starts_with("Fortran") && !parts[i].starts_with("ASM") {
                            languages.push(parts[i].to_string());
                            i += 1;
                        }
                        while i < parts.len() && !parts[i].chars().all(|c| c.is_uppercase() || c.is_ascii_punctuation()) {
                            languages.push(parts[i].to_string());
                            i += 1;
                        }
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
            continue;
        }

        // add_executable(target src1 src2 ...)
        if trimmed.starts_with("add_executable(") {
            let args = extract_cmake_args(trimmed, "add_executable(");
            if let Some(target_name) = args.first() {
                targets.push(BuildTarget {
                    name: target_name.clone(),
                    kind: "executable".to_string(),
                    source_path: args.get(1).cloned(),
                });
            }
            continue;
        }

        // add_library(target [STATIC|SHARED|MODULE] src1 ...)
        if trimmed.starts_with("add_library(") {
            let args = extract_cmake_args(trimmed, "add_library(");
            if let Some(target_name) = args.first() {
                let kind = args.get(1).map(|s| s.to_lowercase()).unwrap_or_else(|| "library".to_string());
                targets.push(BuildTarget {
                    name: target_name.clone(),
                    kind,
                    source_path: args.get(2).cloned().or_else(|| args.get(1).cloned()),
                });
            }
            continue;
        }

        // find_package(PkgName REQUIRED)
        if trimmed.starts_with("find_package(") {
            let args = extract_cmake_args(trimmed, "find_package(");
            if let Some(pkg) = args.first() {
                dependencies.push(Dependency {
                    name: pkg.clone(),
                    version_req: None,
                    kind: "normal".to_string(),
                    source: "system".to_string(),
                    source_url: None,
                });
            }
            continue;
        }

        // add_subdirectory(dir)
        if trimmed.starts_with("add_subdirectory(") {
            let args = extract_cmake_args(trimmed, "add_subdirectory(");
            if let Some(dir) = args.first() {
                subdirs.push(dir.clone());
            }
            continue;
        }

        // enable_testing()
        if trimmed.starts_with("enable_testing(") {
            has_tests = true;
            continue;
        }

        // add_test(NAME name COMMAND cmd)
        if trimmed.starts_with("add_test(") {
            has_tests = true;
            continue;
        }

        // find_library, set, target_link_libraries, etc.
        // We skip these for now as they're complex
    }

    // Build workspace members from subdirs that have CMakeLists.txt
    let workspace_members: Vec<WorkspaceMember> = subdirs
        .iter()
        .filter(|dir| project_root.join(dir).join("CMakeLists.txt").exists())
        .map(|dir| WorkspaceMember {
            name: dir.clone(),
            path: dir.clone(),
            version: None,
        })
        .collect();

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: "cmake_project".to_string(),
        build_system: "CMake".to_string(),
        is_workspace: !workspace_members.is_empty(),
        workspace_members,
        scripts: vec![
            BuildScript { name: "configure".to_string(), command: "cmake -B build".to_string() },
            BuildScript { name: "build".to_string(), command: "cmake --build build".to_string() },
            BuildScript { name: "test".to_string(), command: "ctest --test-dir build".to_string() },
        ],
        features: vec![],
        targets,
        config_files: vec!["CMakeLists.txt".to_string()],
        raw: None,
    })
}

/// Extract arguments from a CMake function call.
fn extract_cmake_args(line: &str, func_prefix: &str) -> Vec<String> {
    let mut args = Vec::new();
    let start = func_prefix.len();
    if start >= line.len() {
        return args;
    }

    let content = &line[start..];
    let mut depth = 0;
    let mut current = String::new();
    let mut in_string = false;

    for ch in content.chars() {
        match ch {
            '(' => {
                depth += 1;
                if depth > 1 {
                    current.push(ch);
                }
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if !current.trim().is_empty() {
                        args.push(current.trim().to_string());
                    }
                    break;
                }
                current.push(ch);
            }
            '"' => {
                in_string = !in_string;
                current.push(ch);
            }
            ' ' | '\t' => {
                if in_string {
                    current.push(ch);
                } else if !current.is_empty() {
                    args.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    args
}
