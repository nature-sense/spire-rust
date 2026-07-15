// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Gradle build system parser (build.gradle / build.gradle.kts).

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a Gradle build file and return normalized BuildMetadata.
pub fn parse_gradle(project_root: &Path, build_file: &str) -> Option<BuildMetadata> {
    let path = project_root.join(build_file);
    let content = std::fs::read_to_string(&path).ok()?;

    let mut project_name = None;
    let mut version = None;
    let mut has_application = false;
    let mut has_library = false;
    let mut has_spring_boot = false;
    let mut has_android = false;
    let mut has_kotlin = false;
    let mut has_java = false;
    let mut subprojects = Vec::new();

    // Detect if it's Kotlin DSL
    let is_kotlin_dsl = build_file.ends_with(".kts");

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect plugins
        if trimmed.contains("id ") && trimmed.contains("application") {
            has_application = true;
        }
        if trimmed.contains("id ") && trimmed.contains("java-library") || trimmed.contains("id ") && trimmed.contains("java") {
            has_java = true;
        }
        if trimmed.contains("id ") && trimmed.contains("org.jetbrains.kotlin") {
            has_kotlin = true;
        }
        if trimmed.contains("id ") && trimmed.contains("org.springframework.boot") {
            has_spring_boot = true;
        }
        if trimmed.contains("id ") && trimmed.contains("com.android") {
            has_android = true;
        }

        // Extract project name (rootProject.name)
        if trimmed.contains("rootProject.name") {
            if let Some(val) = trimmed.split('=').nth(1) {
                project_name = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
            }
        }

        // Extract version
        if trimmed.starts_with("version ") && !trimmed.contains('=') {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                version = Some(parts[1].trim_matches('"').trim_matches('\'').to_string());
            }
        }

        // Detect subprojects
        if trimmed.starts_with("include(") || trimmed.starts_with("include ") {
            // Extract subproject names
            let args = trimmed
                .trim_start_matches("include(")
                .trim_start_matches("include ")
                .trim_end_matches(')');
            for arg in args.split(',') {
                let name = arg.trim().trim_matches('"').trim_matches('\'').trim();
                if !name.is_empty() {
                    subprojects.push(name.to_string());
                }
            }
        }
    }

    // Determine project type
    let project_type = if has_android {
        "android_project"
    } else if has_spring_boot {
        "spring_boot_project"
    } else if has_application {
        "gradle_application"
    } else if has_library {
        "gradle_library"
    } else if has_kotlin {
        "kotlin_project"
    } else {
        "gradle_project"
    };

    let build_system = if is_kotlin_dsl { "Gradle Kotlin DSL" } else { "Gradle Groovy DSL" };

    let mut scripts = Vec::new();
    scripts.push(BuildScript {
        name: "build".to_string(),
        command: "./gradlew build".to_string(),
    });
    scripts.push(BuildScript {
        name: "test".to_string(),
        command: "./gradlew test".to_string(),
    });
    scripts.push(BuildScript {
        name: "clean".to_string(),
        command: "./gradlew clean".to_string(),
    });

    let workspace_members: Vec<WorkspaceMember> = subprojects
        .iter()
        .map(|s| WorkspaceMember {
            name: s.clone(),
            path: s.clone(),
            version: None,
        })
        .collect();

    Some(BuildMetadata {
        project_name,
        version,
        project_type: project_type.to_string(),
        build_system: build_system.to_string(),
        is_workspace: !subprojects.is_empty(),
        workspace_members,
        scripts,
        features: vec![],
        targets: vec![],
        config_files: vec![build_file.to_string()],
        raw: None,
    })
}
