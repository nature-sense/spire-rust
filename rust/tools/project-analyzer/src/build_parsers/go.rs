use std::path::Path;

use crate::models::*;

/// Parse a go.mod file and return normalized BuildMetadata.
pub fn parse_go_mod(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("go.mod");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut go_version: Option<String> = None;

    let mut dependencies = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // module example.com/my/module
        if let Some(val) = trimmed.strip_prefix("module ") {
            name = Some(val.trim().to_string());
            continue;
        }

        // go 1.21
        if let Some(val) = trimmed.strip_prefix("go ") {
            go_version = Some(val.trim().to_string());
            continue;
        }

        // require (
        //     example.com/dep v1.0.0
        // )
        if trimmed.starts_with("require ") {
            // Inline: require example.com/dep v1.0.0
            let req = trimmed.strip_prefix("require ").unwrap_or("").trim();
            if !req.is_empty() && !req.starts_with('(') {
                let parts: Vec<&str> = req.split_whitespace().collect();
                if parts.len() >= 2 {
                    dependencies.push(Dependency {
                        name: parts[0].to_string(),
                        version_req: Some(parts[1].to_string()),
                        kind: "normal".to_string(),
                        source: "registry".to_string(),
                        source_url: None,
                    });
                }
            }
            continue;
        }

        // Inside require block
        if !trimmed.starts_with("//") && !trimmed.starts_with("require") && !trimmed.starts_with("exclude") && !trimmed.starts_with("replace") && !trimmed.starts_with("retract") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 && !parts[0].starts_with('(') && !parts[0].starts_with(')') {
                dependencies.push(Dependency {
                    name: parts[0].to_string(),
                    version_req: Some(parts[1].to_string()),
                    kind: "normal".to_string(),
                    source: "registry".to_string(),
                    source_url: None,
                });
            }
        }
    }

    Some(BuildMetadata {
        project_name: name,
        version: go_version,
        project_type: "go_module".to_string(),
        build_system: "Go Modules".to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts: vec![
            BuildScript { name: "build".to_string(), command: "go build ./...".to_string() },
            BuildScript { name: "test".to_string(), command: "go test ./...".to_string() },
            BuildScript { name: "fmt".to_string(), command: "go fmt ./...".to_string() },
            BuildScript { name: "vet".to_string(), command: "go vet ./...".to_string() },
            BuildScript { name: "tidy".to_string(), command: "go mod tidy".to_string() },
        ],
        features: vec![],
        targets: vec![],
        config_files: vec!["go.mod".to_string()],
        raw: None,
    })
}
