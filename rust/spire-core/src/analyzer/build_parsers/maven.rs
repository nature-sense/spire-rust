// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Maven build system parser (pom.xml).

use std::path::Path;

use crate::analyzer::models::*;

/// Parse a pom.xml file and return normalized BuildMetadata.
pub fn parse_pom(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("pom.xml");
    let content = std::fs::read_to_string(&path).ok()?;

    // Simple XML parsing using basic string operations
    let mut name = None;
    let mut version = None;
    let mut group_id = None;
    let mut artifact_id = None;
    let mut packaging = None;
    let mut modules = Vec::new();
    let mut properties = std::collections::HashMap::new();

    // Extract project coordinates
    if let Some(g) = extract_xml_tag(&content, "groupId") {
        group_id = Some(g);
    }
    if let Some(a) = extract_xml_tag(&content, "artifactId") {
        artifact_id = Some(a);
    }
    if let Some(v) = extract_xml_tag(&content, "version") {
        version = Some(v);
    }
    if let Some(p) = extract_xml_tag(&content, "packaging") {
        packaging = Some(p);
    }

    // Extract name
    if let Some(n) = extract_xml_tag(&content, "name") {
        name = Some(n);
    }

    // Extract modules
    if let Some(modules_content) = extract_xml_block(&content, "modules") {
        for line in modules_content.lines() {
            let trimmed = line.trim();
            if let Some(module) = trimmed.strip_prefix("<module>").and_then(|s| s.strip_suffix("</module>")) {
                modules.push(module.trim().to_string());
            }
        }
    }

    // Extract properties
    if let Some(props_content) = extract_xml_block(&content, "properties") {
        for line in props_content.lines() {
            let trimmed = line.trim();
            if let Some(tag_end) = trimmed.find('>') {
                if let Some(tag_start) = trimmed.find('<') {
                    let key = trimmed[tag_start + 1..tag_end].to_string();
                    if !key.starts_with('/') && !key.starts_with('?') {
                        let value = trimmed[tag_end + 1..]
                            .trim_end_matches("</")
                            .trim_end_matches(&key)
                            .trim_end_matches('>')
                            .to_string();
                        properties.insert(key, value);
                    }
                }
            }
        }
    }

    // Determine project type
    let project_type = match packaging.as_deref() {
        Some("pom") => "maven_parent",
        Some("jar") => "maven_library",
        Some("war") => "maven_webapp",
        Some("ear") => "maven_enterprise_app",
        Some("bundle") => "maven_bundle",
        _ => "maven_project",
    };

    let workspace_members: Vec<WorkspaceMember> = modules
        .iter()
        .map(|m| WorkspaceMember {
            name: m.clone(),
            path: m.clone(),
            version: version.clone(),
        })
        .collect();

    let project_name = name.or_else(|| artifact_id.clone());

    Some(BuildMetadata {
        project_name,
        version,
        project_type: project_type.to_string(),
        build_system: "Maven".to_string(),
        is_workspace: !modules.is_empty(),
        workspace_members,
        scripts: vec![
            BuildScript { name: "compile".to_string(), command: "mvn compile".to_string() },
            BuildScript { name: "test".to_string(), command: "mvn test".to_string() },
            BuildScript { name: "package".to_string(), command: "mvn package".to_string() },
            BuildScript { name: "clean".to_string(), command: "mvn clean".to_string() },
            BuildScript { name: "install".to_string(), command: "mvn install".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec!["pom.xml".to_string()],
        raw: None,
    })
}

/// Extract the content of an XML tag (first occurrence).
fn extract_xml_tag(content: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    if let Some(start) = content.find(&open) {
        let value_start = start + open.len();
        if let Some(end) = content[value_start..].find(&close) {
            let value = content[value_start..value_start + end].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Extract the content of an XML block (between opening and closing tags).
fn extract_xml_block(content: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    if let Some(start) = content.find(&open) {
        let value_start = start + open.len();
        if let Some(end) = content[value_start..].find(&close) {
            return Some(content[value_start..value_start + end].to_string());
        }
    }

    None
}
