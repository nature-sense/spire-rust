use std::path::Path;

use crate::models::*;

/// Parse a Maven pom.xml and return normalized BuildMetadata.
///
/// Uses simple XML tag parsing (no full XML parser needed for the key fields).
pub fn parse_pom(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("pom.xml");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;
    let mut group_id = None;
    let mut artifact_id = None;
    let mut packaging = None;
    let mut dependencies = Vec::new();
    let mut modules = Vec::new();
    let mut properties = Vec::new();
    let mut in_dependencies = false;
    let mut in_dependency = false;
    let mut in_modules = false;
    let mut in_module = false;
    let mut in_properties = false;
    let mut in_build = false;
    let mut in_plugins = false;
    let mut current_dep = Dependency {
        name: String::new(),
        version_req: None,
        kind: "normal".to_string(),
        source: "registry".to_string(),
        source_url: None,
    };
    let mut dep_group = String::new();
    let mut dep_artifact = String::new();
    let mut dep_version = String::new();
    let mut dep_scope = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Track state
        if trimmed.contains("<dependencies>") {
            in_dependencies = true;
            continue;
        }
        if trimmed.contains("</dependencies>") {
            in_dependencies = false;
            continue;
        }
        if trimmed.contains("<dependency>") {
            in_dependency = true;
            dep_group.clear();
            dep_artifact.clear();
            dep_version.clear();
            dep_scope.clear();
            continue;
        }
        if trimmed.contains("</dependency>") {
            if in_dependency && !dep_group.is_empty() && !dep_artifact.is_empty() {
                let dep_name = format!("{}:{}", dep_group, dep_artifact);
                current_dep = Dependency {
                    name: dep_name,
                    version_req: if dep_version.is_empty() { None } else { Some(dep_version.clone()) },
                    kind: if dep_scope.is_empty() { "normal".to_string() } else { dep_scope.clone() },
                    source: "registry".to_string(),
                    source_url: None,
                };
                dependencies.push(current_dep.clone());
            }
            in_dependency = false;
            continue;
        }

        if trimmed.contains("<modules>") {
            in_modules = true;
            continue;
        }
        if trimmed.contains("</modules>") {
            in_modules = false;
            continue;
        }

        if trimmed.contains("<properties>") {
            in_properties = true;
            continue;
        }
        if trimmed.contains("</properties>") {
            in_properties = false;
            continue;
        }

        if trimmed.contains("<build>") {
            in_build = true;
            continue;
        }
        if trimmed.contains("</build>") {
            in_build = false;
            continue;
        }
        if trimmed.contains("<plugins>") || trimmed.contains("</plugins>") {
            continue;
        }

        // Extract values
        if let Some(val) = extract_xml_tag(trimmed, "groupId") {
            if in_dependency {
                dep_group = val;
            } else {
                group_id = Some(val);
            }
            continue;
        }
        if let Some(val) = extract_xml_tag(trimmed, "artifactId") {
            if in_dependency {
                dep_artifact = val;
            } else {
                artifact_id = Some(val);
            }
            continue;
        }
        if let Some(val) = extract_xml_tag(trimmed, "version") {
            if in_dependency {
                dep_version = val;
            } else if !in_properties && !in_build {
                version = Some(val);
            }
            continue;
        }
        if let Some(val) = extract_xml_tag(trimmed, "packaging") {
            packaging = Some(val);
            continue;
        }
        if let Some(val) = extract_xml_tag(trimmed, "scope") {
            dep_scope = val;
            continue;
        }
        if let Some(val) = extract_xml_tag(trimmed, "name") {
            if !in_dependency && !in_modules && !in_properties && !in_build {
                name = Some(val);
            }
            continue;
        }

        // Module entries
        if in_modules {
            if let Some(val) = extract_xml_tag(trimmed, "module") {
                modules.push(val);
                continue;
            }
        }

        // Properties
        if in_properties {
            // Simple property: <key>value</key>
            if let Some(close) = trimmed.find("</") {
                let key = trimmed.trim_start_matches('<').split('>').next().unwrap_or("").to_string();
                let val = trimmed.split('>').nth(1).and_then(|s| s.split("</").next()).unwrap_or("").to_string();
                if !key.is_empty() && !val.is_empty() {
                    properties.push(Feature {
                        name: key,
                        description: None,
                        default: false,
                    });
                }
            }
        }
    }

    // Build project name from group:artifact
    let project_name = name.or_else(|| {
        match (&group_id, &artifact_id) {
            (Some(g), Some(a)) => Some(format!("{}:{}", g, a)),
            (_, Some(a)) => Some(a.clone()),
            _ => None,
        }
    });

    // Detect project type from packaging
    let project_type = match packaging.as_deref() {
        Some("pom") => "maven_multi_module",
        Some("war") => "maven_webapp",
        Some("jar") => "maven_library",
        _ => "maven_project",
    };

    Some(BuildMetadata {
        project_name,
        version,
        project_type: project_type.to_string(),
        build_system: "Maven".to_string(),
        is_workspace: !modules.is_empty(),
        workspace_members: modules.iter().map(|m| WorkspaceMember {
            name: m.clone(),
            path: m.clone(),
            version: None,
        }).collect(),
        scripts: vec![
            BuildScript { name: "compile".to_string(), command: "mvn compile".to_string() },
            BuildScript { name: "test".to_string(), command: "mvn test".to_string() },
            BuildScript { name: "package".to_string(), command: "mvn package".to_string() },
            BuildScript { name: "clean".to_string(), command: "mvn clean".to_string() },
            BuildScript { name: "install".to_string(), command: "mvn install".to_string() },
        ],
        features: properties,
        targets: vec![],
        config_files: vec!["pom.xml".to_string()],
        raw: None,
    })
}

/// Extract the value of an XML tag: <tagName>value</tagName>
fn extract_xml_tag(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    if let Some(start) = line.find(&open) {
        let value_start = start + open.len();
        if let Some(end) = line[value_start..].find(&close) {
            let value = line[value_start..value_start + end].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
