use std::path::Path;

use crate::models::*;

/// Parse a Makefile and return normalized BuildMetadata.
///
/// Makefiles are the most generic build system. This parser extracts:
/// - Target names (lines starting with a word followed by ':')
/// - Variable assignments
/// - Included makefiles
pub fn parse_makefile(project_root: &Path) -> Option<BuildMetadata> {
    let path = project_root.join("Makefile");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut targets = Vec::new();
    let mut variables = Vec::new();
    let mut has_tests = false;
    let mut has_install = false;
    let mut has_clean = false;
    let mut has_build = false;
    let mut has_all = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        // Variable assignment: VAR = value or VAR := value
        if let Some(eq_pos) = trimmed.find('=') {
            // Make sure it's not a target (targets don't have = before :)
            if !trimmed.contains(":=") && !trimmed.contains("= ") {
                // Could be a target with = in the recipe, skip
            }
            let var_name = trimmed[..eq_pos].trim();
            if !var_name.is_empty() && !var_name.contains(' ') && !var_name.contains(':') {
                variables.push(Feature {
                    name: var_name.to_string(),
                    description: None,
                    default: false,
                });
            }
            continue;
        }

        // Target definition: target: [prerequisites...]
        // Also handle pattern rules: %.o: %.c
        if let Some(colon_pos) = trimmed.find(':') {
            let before_colon = trimmed[..colon_pos].trim();
            // Skip if it contains shell syntax or is a variable reference
            if !before_colon.contains('$') && !before_colon.starts_with('.') && !before_colon.is_empty() {
                let target_name = before_colon.split_whitespace().next().unwrap_or("").to_string();
                if !target_name.is_empty() {
                    // Check for special targets
                    match target_name.as_str() {
                        "all" => has_all = true,
                        "build" => has_build = true,
                        "test" | "tests" | "check" => has_tests = true,
                        "install" => has_install = true,
                        "clean" | "distclean" => has_clean = true,
                        _ => {}
                    }
                    targets.push(BuildTarget {
                        name: target_name,
                        kind: "make_target".to_string(),
                        source_path: None,
                    });
                }
            }
            continue;
        }

        // include other makefiles
        if trimmed.starts_with("include ") {
            // Not extracting included makefiles for now
            continue;
        }
    }

    // Build scripts from targets
    let mut scripts = Vec::new();
    if has_all || has_build {
        scripts.push(BuildScript {
            name: "build".to_string(),
            command: "make".to_string(),
        });
    }
    if has_tests {
        scripts.push(BuildScript {
            name: "test".to_string(),
            command: "make test".to_string(),
        });
    }
    if has_clean {
        scripts.push(BuildScript {
            name: "clean".to_string(),
            command: "make clean".to_string(),
        });
    }
    if has_install {
        scripts.push(BuildScript {
            name: "install".to_string(),
            command: "make install".to_string(),
        });
    }
    // Default scripts if nothing specific found
    if scripts.is_empty() {
        scripts.push(BuildScript {
            name: "build".to_string(),
            command: "make".to_string(),
        });
        scripts.push(BuildScript {
            name: "clean".to_string(),
            command: "make clean".to_string(),
        });
    }

    Some(BuildMetadata {
        project_name: None,
        version: None,
        project_type: "make_project".to_string(),
        build_system: "Make".to_string(),
        is_workspace: false,
        workspace_members: vec![],
        scripts,
        features: variables,
        targets,
        config_files: vec!["Makefile".to_string()],
        raw: None,
    })
}
