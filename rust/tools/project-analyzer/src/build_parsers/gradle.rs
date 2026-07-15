use std::path::Path;

use crate::models::*;

/// Parse a Gradle build file and return normalized BuildMetadata.
///
/// Gradle uses Groovy DSL (build.gradle) or Kotlin DSL (build.gradle.kts).
/// This parser does simple line-based extraction of key metadata.
pub fn parse_gradle(project_root: &Path, build_file: &str) -> Option<BuildMetadata> {
    let path = project_root.join(build_file);
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name = None;
    let mut version = None;
    let mut group = None;
    let mut plugins = Vec::new();
    let mut dependencies = Vec::new();
    let mut has_application = false;
    let mut has_library = false;
    let mut has_spring_boot = false;
    let mut has_android = false;
    let mut has_kotlin = false;
    let mut has_java = false;
    let mut tasks = Vec::new();

    let is_kotlin_dsl = build_file.ends_with(".kts");

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
            continue;
        }

        // rootProject.name = "my-project" (settings.gradle)
        if let Some(val) = trimmed.strip_prefix("rootProject.name ") {
            let parts: Vec<&str> = val.split('=').collect();
            if parts.len() >= 2 {
                name = Some(parts[1].trim().trim_matches('"').to_string());
            }
            continue;
        }

        // Plugins block
        if trimmed.starts_with("id ") || trimmed.starts_with("id(") {
            let id_val = if is_kotlin_dsl {
                // id("com.android.application")
                if let Some(val) = trimmed.strip_prefix("id(") {
                    val.trim().trim_matches('"').trim_matches(')').to_string()
                } else {
                    continue;
                }
            } else {
                // id 'com.android.application'
                if let Some(val) = trimmed.strip_prefix("id ") {
                    val.trim().trim_matches('\'').trim_matches('"').to_string()
                } else {
                    continue;
                }
            };

            plugins.push(id_val.clone());
            if id_val.contains("application") {
                has_application = true;
            }
            if id_val.contains("library") {
                has_library = true;
            }
            if id_val.contains("spring-boot") || id_val.contains("spring.boot") {
                has_spring_boot = true;
            }
            if id_val.contains("android") {
                has_android = true;
            }
            if id_val.contains("kotlin") {
                has_kotlin = true;
            }
            if id_val.contains("java") {
                has_java = true;
            }
            continue;
        }

        // group = 'com.example'
        if let Some(val) = trimmed.strip_prefix("group ") {
            let parts: Vec<&str> = val.split('=').collect();
            if parts.len() >= 2 {
                group = Some(parts[1].trim().trim_matches('\'').trim_matches('"').to_string());
            }
            continue;
        }

        // version = '1.0.0'
        if let Some(val) = trimmed.strip_prefix("version ") {
            let parts: Vec<&str> = val.split('=').collect();
            if parts.len() >= 2 {
                version = Some(parts[1].trim().trim_matches('\'').trim_matches('"').to_string());
            }
            continue;
        }

        // implementation 'com.example:lib:1.0'
        // implementation("com.example:lib:1.0")
        for prefix in &["implementation ", "api ", "compileOnly ", "runtimeOnly ", "testImplementation ", "androidTestImplementation ", "kapt ", "annotationProcessor "] {
            if trimmed.starts_with(prefix) {
                let dep_str = trimmed.strip_prefix(prefix).unwrap_or("").trim();
                let clean = dep_str.trim_matches('\'').trim_matches('"');
                // Handle both 'group:name:version' and "group:name:version"
                let parts: Vec<&str> = clean.split(':').collect();
                if parts.len() >= 2 {
                    let dep_name = format!("{}:{}", parts[0], parts[1]);
                    let kind = prefix.trim_end_matches(' ').to_string();
                    dependencies.push(Dependency {
                        name: dep_name,
                        version_req: parts.get(2).map(|s| s.to_string()),
                        kind,
                        source: "registry".to_string(),
                        source_url: None,
                    });
                }
                break;
            }
        }

        // tasks.register<...> or task ... (type: ...)
        if trimmed.starts_with("task ") || trimmed.starts_with("tasks.register") || trimmed.starts_with("tasks.named") {
            // Extract task name
            let task_name = if is_kotlin_dsl {
                if let Some(val) = trimmed.strip_prefix("tasks.register<") {
                    val.split('>').next().map(|s| s.to_string())
                } else if let Some(val) = trimmed.strip_prefix("tasks.named<") {
                    val.split('>').next().map(|s| s.to_string())
                } else {
                    None
                }
            } else {
                if let Some(val) = trimmed.strip_prefix("task ") {
                    Some(val.split_whitespace().next().unwrap_or("").to_string())
                } else {
                    None
                }
            };
            if let Some(tn) = task_name {
                let tn_clone = tn.clone();
                tasks.push(BuildScript {
                    name: tn,
                    command: format!("gradle {} {}", if is_kotlin_dsl { "" } else { "" }, tn_clone),

                });
            }
        }
    }

    // Detect project type
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
    } else if has_java {
        "java_project"
    } else {
        "gradle_project"
    };

    // Detect build system variant
    let build_system = if is_kotlin_dsl {
        "Gradle Kotlin DSL"
    } else {
        "Gradle Groovy DSL"
    };

    Some(BuildMetadata {
        project_name: name.or_else(|| group.clone()),
        version,
        project_type: project_type.to_string(),
        build_system: build_system.to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts: vec![
            BuildScript { name: "build".to_string(), command: "./gradlew build".to_string() },
            BuildScript { name: "test".to_string(), command: "./gradlew test".to_string() },
            BuildScript { name: "clean".to_string(), command: "./gradlew clean".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec![build_file.to_string()],
        raw: None,
    })
}
