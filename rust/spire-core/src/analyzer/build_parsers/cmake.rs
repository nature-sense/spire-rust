// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! CMake build system parser (CMakeLists.txt).

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a CMakeLists.txt file and return normalized BuildMetadata.
pub fn parse_cmake(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("CMakeLists.txt");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut project_name = None;
    let mut version = None;
    let mut languages = Vec::new();
    let mut cmake_minimum = None;
    let mut targets = Vec::new();
    let mut subdirectories = Vec::new();
    let mut has_tests = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // cmake_minimum_required(VERSION 3.10)
        if trimmed.starts_with("cmake_minimum_required") {
            if let Some(ver) = trimmed.split("VERSION").nth(1) {
                cmake_minimum = Some(ver.trim().trim_end_matches(')').to_string());
            }
        }

        // project(MyProject VERSION 1.0 LANGUAGES C CXX)
        if trimmed.starts_with("project(") {
            let args_str = trimmed
                .trim_start_matches("project(")
                .trim_end_matches(')');

            // First argument is the project name
            let args: Vec<&str> = args_str.split_whitespace().collect();
            if let Some(first) = args.first() {
                let name = first.trim_matches('"').trim_matches('\'');
                if !name.is_empty() && !name.starts_with("VERSION") && !name.starts_with("LANGUAGES") {
                    project_name = Some(name.to_string());
                }
            }

            // Extract VERSION and LANGUAGES
            let mut i = 0;
            while i < args.len() {
                if args[i] == "VERSION" {
                    if let Some(v) = args.get(i + 1) {
                        version = Some(v.trim_matches('"').trim_matches('\'').to_string());
                    }
                    i += 2;
                } else if args[i] == "LANGUAGES" {
                    i += 1;
                    while i < args.len() && !args[i].starts_with("VERSION") && !args[i].starts_with("LANGUAGES") {
                        languages.push(args[i].trim_matches('"').trim_matches('\'').to_string());
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }

        // add_executable(name source1 source2 ...)
        if trimmed.starts_with("add_executable(") {
            let args_str = trimmed
                .trim_start_matches("add_executable(")
                .trim_end_matches(')');
            let args: Vec<&str> = args_str.split_whitespace().collect();
            if let Some(name) = args.first() {
                let target_name = name.trim_matches('"').trim_matches('\'');
                targets.push(BuildTarget {
                    name: target_name.to_string(),
                    kind: "executable".to_string(),
                    source_path: args.get(1).map(|s| s.trim_matches('"').trim_matches('\'').to_string()),
                });
            }
        }

        // add_library(name source1 source2 ...)
        if trimmed.starts_with("add_library(") {
            let args_str = trimmed
                .trim_start_matches("add_library(")
                .trim_end_matches(')');
            let args: Vec<&str> = args_str.split_whitespace().collect();
            if let Some(name) = args.first() {
                let target_name = name.trim_matches('"').trim_matches('\'');
                targets.push(BuildTarget {
                    name: target_name.to_string(),
                    kind: "library".to_string(),
                    source_path: args.get(1).map(|s| s.trim_matches('"').trim_matches('\'').to_string()),
                });
            }
        }

        // add_subdirectory(dir)
        if trimmed.starts_with("add_subdirectory(") {
            let args_str = trimmed
                .trim_start_matches("add_subdirectory(")
                .trim_end_matches(')');
            let dir = args_str.split_whitespace().next().unwrap_or("");
            let dir_name = dir.trim_matches('"').trim_matches('\'');
            if !dir_name.is_empty() {
                subdirectories.push(dir_name.to_string());
            }
        }

        // enable_testing() / add_test()
        if trimmed.starts_with("enable_testing()") || trimmed.starts_with("add_test(") {
            has_tests = true;
        }
    }

    let workspace_members: Vec<WorkspaceMember> = subdirectories
        .iter()
        .map(|s| WorkspaceMember {
            name: s.clone(),
            path: s.clone(),
            version: None,
        })
        .collect();

    let mut scripts = Vec::new();
    scripts.push(BuildScript {
        name: "configure".to_string(),
        command: "cmake -B build".to_string(),
    });
    scripts.push(BuildScript {
        name: "build".to_string(),
        command: "cmake --build build".to_string(),
    });
    if has_tests {
        scripts.push(BuildScript {
            name: "test".to_string(),
            command: "ctest --test-dir build".to_string(),
        });
    }

    Some(BuildMetadata {
        project_name,
        version,
        project_type: "cmake_project".to_string(),
        build_system: "CMake".to_string(),
        is_workspace: !subdirectories.is_empty(),
        workspace_members,
        scripts,
        features: vec![],
        targets,
        config_files: vec!["CMakeLists.txt".to_string()],
        raw: None,
    })
}
