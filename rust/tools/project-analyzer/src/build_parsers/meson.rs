use std::path::Path;

use crate::models::*;

/// Parse a meson.build file and return normalized BuildMetadata.
///
/// Meson is a build system that uses Python-like DSL in `meson.build` files.
/// This parser extracts:
/// - Project name, version, languages from `project()`
/// - Build targets from `executable()`, `library()`, `shared_library()`, `static_library()`
/// - Dependencies from `dependency()`
/// - Sub-projects from `subdir()` and `subproject()`
/// - Build options from `meson.options` / `meson_options.txt`
/// - Wrap dependencies from `subprojects/` directory
pub fn parse_meson_build(project_root: &Path) -> Option<BuildMetadata> {
    let meson_path = project_root.join("meson.build");
    let content = std::fs::read_to_string(&meson_path).ok()?;

    let mut name = None;
    let mut version = None;
    let mut languages = Vec::new();
    let mut targets = Vec::new();
    let mut dependencies = Vec::new();
    let mut subdirs = Vec::new();
    let mut subprojects = Vec::new();
    let mut has_tests = false;
    let mut has_benchmarks = false;

    // Parse line by line (Meson DSL is Python-like, we do simple regex-free parsing)
    for line in content.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        // project('name', 'c', version: '1.0')
        if trimmed.starts_with("project(") {
            let args = extract_args(trimmed, "project(");
            if let Some(first) = args.first() {
                name = Some(first.trim_matches('\'').to_string());
            }
            // Second positional arg is usually the language
            if args.len() > 1 {
                let lang = args[1].trim_matches('\'');
                if !lang.starts_with("version") && !lang.starts_with("meson_version") && !lang.starts_with("default_options") {
                    languages.push(lang.to_string());
                }
            }
            // Extract keyword arguments
            for arg in &args {
                if let Some(val) = arg.strip_prefix("version:") {
                    version = Some(val.trim().trim_matches('\'').to_string());
                }
                if let Some(val) = arg.strip_prefix("meson_version:") {
                    // Just note it, not critical
                }
            }
            continue;
        }

        // executable('name', 'src1.c', 'src2.c')
        if trimmed.starts_with("executable(") {
            let args = extract_args(trimmed, "executable(");
            if let Some(target_name) = args.first() {
                let src_files: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !a.starts_with('#'))
                    .map(|a| a.trim_matches('\'').to_string())
                    .collect();
                targets.push(BuildTarget {
                    name: target_name.trim_matches('\'').to_string(),
                    kind: "executable".to_string(),
                    source_path: src_files.first().cloned(),
                });
            }
            continue;
        }

        // library('name', ...), shared_library('name', ...), static_library('name', ...)
        if trimmed.starts_with("library(") || trimmed.starts_with("shared_library(") || trimmed.starts_with("static_library(") {
            let func_name = if trimmed.starts_with("shared_library(") {
                "shared_library("
            } else if trimmed.starts_with("static_library(") {
                "static_library("
            } else {
                "library("
            };
            let args = extract_args(trimmed, func_name);
            if let Some(target_name) = args.first() {
                let kind = match func_name {
                    "shared_library(" => "shared_library",
                    "static_library(" => "static_library",
                    _ => "library",
                };
                let src_files: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !a.starts_with('#'))
                    .map(|a| a.trim_matches('\'').to_string())
                    .collect();
                targets.push(BuildTarget {
                    name: target_name.trim_matches('\'').to_string(),
                    kind: kind.to_string(),
                    source_path: src_files.first().cloned(),
                });
            }
            continue;
        }

        // dependency('name')
        if trimmed.starts_with("dependency(") {
            let args = extract_args(trimmed, "dependency(");
            if let Some(dep_name) = args.first() {
                let clean_name = dep_name.trim_matches('\'').to_string();
                // Check if it's a wrap dependency (subprojects)
                let is_wrap = is_wrap_dependency(project_root, &clean_name);
                dependencies.push(Dependency {
                    name: clean_name,
                    version_req: None,
                    kind: "normal".to_string(),
                    source: if is_wrap { "wrap" } else { "system" }.to_string(),
                    source_url: None,
                });
            }
            continue;
        }

        // subdir('dirname')
        if trimmed.starts_with("subdir(") {
            let args = extract_args(trimmed, "subdir(");
            if let Some(dir) = args.first() {
                subdirs.push(dir.trim_matches('\'').to_string());
            }
            continue;
        }

        // subproject('name')
        if trimmed.starts_with("subproject(") {
            let args = extract_args(trimmed, "subproject(");
            if let Some(sp) = args.first() {
                subprojects.push(sp.trim_matches('\'').to_string());
            }
            continue;
        }

        // test('name', executable)
        if trimmed.starts_with("test(") {
            has_tests = true;
            continue;
        }

        // benchmark('name', executable)
        if trimmed.starts_with("benchmark(") {
            has_benchmarks = true;
            continue;
        }

        // import('fs'), import('python'), etc.
        // gnome = import('gnome'), etc.
        if trimmed.starts_with("import(") {
            // Not extracting module imports for now
            continue;
        }
    }

    // Check for meson.options or meson_options.txt
    let features = parse_meson_options(project_root);

    // Check for wrap files in subprojects/
    let wrap_deps = find_wrap_dependencies(project_root);
    for wrap in wrap_deps {
        if !dependencies.iter().any(|d| d.name == wrap) {
            dependencies.push(Dependency {
                name: wrap,
                version_req: None,
                kind: "normal".to_string(),
                source: "wrap".to_string(),
                source_url: None,
            });
        }
    }

    // Build workspace members from subdirs that have their own meson.build
    let workspace_members: Vec<WorkspaceMember> = subdirs
        .iter()
        .filter(|dir| project_root.join(dir).join("meson.build").exists())
        .map(|dir| WorkspaceMember {
            name: dir.clone(),
            path: dir.clone(),
            version: None,
        })
        .collect();

    // Add subprojects as workspace members
    let mut all_members = workspace_members;
    for sp in &subprojects {
        if !all_members.iter().any(|m| m.name == *sp) {
            all_members.push(WorkspaceMember {
                name: sp.clone(),
                path: format!("subprojects/{}", sp),
                version: None,
            });
        }
    }

    // Build scripts
    let mut scripts = Vec::new();
    scripts.push(BuildScript {
        name: "setup".to_string(),
        command: "meson setup builddir".to_string(),
    });
    scripts.push(BuildScript {
        name: "compile".to_string(),
        command: "meson compile -C builddir".to_string(),
    });
    scripts.push(BuildScript {
        name: "test".to_string(),
        command: "meson test -C builddir".to_string(),
    });
    if has_benchmarks {
        scripts.push(BuildScript {
            name: "benchmark".to_string(),
            command: "meson test -C builddir --benchmark".to_string(),
        });
    }
    scripts.push(BuildScript {
        name: "install".to_string(),
        command: "meson install -C builddir".to_string(),
    });

    Some(BuildMetadata {
        project_name: name,
        version,
        project_type: "meson_project".to_string(),
        build_system: "Meson".to_string(),
        is_workspace: !all_members.is_empty(),
        workspace_members: all_members,
        scripts,
        features,
        targets,
        config_files: vec!["meson.build".to_string()],
        raw: None,
    })
}

/// Extract arguments from a Meson function call.
/// Handles simple cases: `func('arg1', 'arg2', key: 'val')`
fn extract_args(line: &str, func_prefix: &str) -> Vec<String> {
    let mut args = Vec::new();
    let start = func_prefix.len();
    if start >= line.len() {
        return args;
    }

    let content = &line[start..];
    let mut depth = 0;
    let mut current = String::new();
    let mut in_string = false;
    let mut string_char = '\'';

    for ch in content.chars() {
        match ch {
            '(' => {
                if depth > 0 {
                    current.push(ch);
                }
                depth += 1;
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
            '\'' | '"' => {
                if in_string {
                    if ch == string_char {
                        in_string = false;
                    }
                    current.push(ch);
                } else {
                    in_string = true;
                    string_char = ch;
                    current.push(ch);
                }
            }
            ',' => {
                if !in_string {
                    if !current.trim().is_empty() {
                        args.push(current.trim().to_string());
                    }
                    current.clear();
                } else {
                    current.push(ch);
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    args
}

/// Parse meson.options or meson_options.txt for feature flags.
fn parse_meson_options(project_root: &Path) -> Vec<Feature> {
    let mut features = Vec::new();

    // Check meson.options first (newer Meson), then meson_options.txt
    let options_paths = [
        project_root.join("meson.options"),
        project_root.join("meson_options.txt"),
    ];

    for path in &options_paths {
        if !path.exists() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    continue;
                }
                // option('name', type: 'boolean', value: true, description: '...')
                if trimmed.starts_with("option(") {
                    let args = extract_args(trimmed, "option(");
                    if let Some(opt_name) = args.first() {
                        let clean_name = opt_name.trim_matches('\'').to_string();
                        let mut description = None;
                        let mut default_val = false;

                        for arg in &args {
                            if let Some(desc) = arg.strip_prefix("description:") {
                                description = Some(desc.trim().trim_matches('\'').to_string());
                            }
                            if arg.contains("value: true") || arg.contains("value : true") {
                                default_val = true;
                            }
                        }

                        features.push(Feature {
                            name: clean_name,
                            description,
                            default: default_val,
                        });
                    }
                }
            }
        }
        // Only try the first found file
        break;
    }

    features
}

/// Check if a dependency name has a corresponding wrap file in subprojects/.
fn is_wrap_dependency(project_root: &Path, dep_name: &str) -> bool {
    let wrap_path = project_root.join("subprojects").join(format!("{}.wrap", dep_name));
    wrap_path.exists()
}

/// Find all wrap files in subprojects/ directory.
fn find_wrap_dependencies(project_root: &Path) -> Vec<String> {
    let subprojects_dir = project_root.join("subprojects");
    if !subprojects_dir.is_dir() {
        return Vec::new();
    }

    let mut wraps = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&subprojects_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "wrap").unwrap_or(false) {
                if let Some(name) = path.file_stem() {
                    wraps.push(name.to_string_lossy().to_string());
                }
            }
        }
    }
    wraps
}
