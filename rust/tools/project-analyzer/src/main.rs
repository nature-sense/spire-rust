use std::path::PathBuf;

use clap::Parser;

mod analyzer;
mod build_parsers;
mod models;
mod rust_analyzer;
mod scanner;
mod tree_builder;


/// Analyze a project directory to determine its type, languages, build tools,
/// and directory structure — giving LLMs semantic understanding of a codebase.
#[derive(Parser, Debug)]
#[command(name = "project-analyzer", version, about)]
struct Args {
    /// Path to the project root directory to analyze.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Output format: "json" (default), "pretty", "tree", or "summary".
    #[arg(long, default_value = "json")]
    format: String,


    /// Do not respect .gitignore files (scan everything).
    #[arg(long)]
    no_ignore: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Canonicalize the path
    let root = if args.path.is_relative() {
        std::env::current_dir()?.join(&args.path)
    } else {
        args.path.clone()
    };

    let root = root.canonicalize().map_err(|e| {
        anyhow::anyhow!("Cannot access path '{}': {}", args.path.display(), e)
    })?;

    if !root.is_dir() {
        anyhow::bail!("'{}' is not a directory", root.display());
    }

    // Run the three-stage analysis
    let analysis = analyzer::analyze_project(&root, args.no_ignore);

    // Output
    match args.format.as_str() {
        "pretty" => {
            print_analysis(&analysis, 0);
        }
        "tree" => {
            // Build the universal file tree and print it
            let tree = tree_builder::build_file_tree(&root, args.no_ignore);
            let json = serde_json::to_string_pretty(&tree)?;
            println!("{}", json);
        }

        "summary" => {
            // Default JSON output (the existing summary format)
            let json = serde_json::to_string_pretty(&analysis)?;
            println!("{}", json);
        }
        _ => {
            // Default: JSON output
            let json = serde_json::to_string_pretty(&analysis)?;
            println!("{}", json);
        }
    }


    Ok(())
}

/// Pretty-print a project analysis, with optional indentation for sub-projects.
fn print_analysis(analysis: &models::ProjectAnalysis, depth: usize) {
    let indent = "  ".repeat(depth);

    if depth == 0 {
        println!("=== Project Analysis ===");
    } else {
        let name = std::path::Path::new(&analysis.root)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        println!("{}--- Sub-Project: {} ---", indent, name);
    }

    println!("{}Root: {}", indent, analysis.root);
    println!("{}Type: {} (confidence: {:.1}%)", indent, analysis.project_type, analysis.confidence * 100.0);
    println!();

    if !analysis.languages.is_empty() {
        println!("{}--- Languages ---", indent);
        for lang in &analysis.languages {
            println!(
                "{}  {}: {} files, ~{} lines ({})",
                indent,
                lang.name,
                lang.file_count,
                lang.estimated_lines,
                lang.extensions.join(", ")
            );
        }
        println!();
    }

    if !analysis.build_tools.is_empty() {
        println!("{}--- Build Tools ---", indent);
        for tool in &analysis.build_tools {
            let ws = tool.is_workspace.map(|w| if w { " (workspace)" } else { "" }).unwrap_or("");
            println!("{}  {}{}", indent, tool.name, ws);
            if !tool.config_files.is_empty() {
                println!("{}    Config: {}", indent, tool.config_files.join(", "));
            }
        }
        println!();
    }

    if !analysis.entry_points.is_empty() {
        println!("{}--- Entry Points ---", indent);
        for entry in &analysis.entry_points {
            println!("{}  {} ({})", indent, entry.path, entry.entry_type);
        }
        println!();
    }

    if !analysis.directory_structure.is_empty() {
        println!("{}--- Directory Structure ---", indent);
        let mut dirs: Vec<(&String, &models::DirEntry)> = analysis.directory_structure.iter().collect();
        dirs.sort_by(|a, b| a.0.cmp(b.0));
        for (name, entry) in &dirs {
            let src = entry.has_src.map(|s| if s { " [src]" } else { "" }).unwrap_or("");
            let tests = entry.has_tests.map(|t| if t { " [tests]" } else { "" }).unwrap_or("");
            let subs = entry.sub_projects.as_ref().map(|s| format!(" [sub: {}]", s.join(", "))).unwrap_or_default();
            println!(
                "{}  {}/: {} ({} files{}{}{})",
                indent,
                name,
                entry.dir_type,
                entry.file_count.unwrap_or(0),
                src,
                tests,
                subs
            );
        }
        println!();
    }

    if !analysis.key_files.is_empty() {
        println!("{}--- Key Files ---", indent);
        for kf in &analysis.key_files {
            println!("{}  {} → {}", indent, kf.path, kf.role);
        }
        println!();
    }

    // ── Stage 1: Build projects ────────────────────────────────────────────
    if !analysis.build_projects.is_empty() {
        println!("{}--- Build Projects (Stage 1) ---", indent);
        for bp in &analysis.build_projects {
            let name_str = bp.name.as_deref().unwrap_or("(unnamed)");
            let ver_str = bp.version.as_deref().map(|v| format!(" v{}", v)).unwrap_or_default();
            let ws_str = bp.is_workspace.map(|w| if w { " [workspace]" } else { "" }).unwrap_or("");
            println!(
                "{}  {}/{}: {}{}{} (conf: {:.0}%)",
                indent,
                bp.root,
                bp.build_file,
                name_str,
                ver_str,
                ws_str,
                bp.confidence * 100.0
            );
            if !bp.workspace_members.is_empty() {
                println!("{}    Members: {}", indent, bp.workspace_members.join(", "));
            }
            if !bp.languages.is_empty() {
                let lang_names: Vec<&str> = bp.languages.iter().map(|l| l.name.as_str()).collect();
                println!("{}    Languages: {}", indent, lang_names.join(", "));
            }
            if !bp.entry_points.is_empty() {
                let eps: Vec<&str> = bp.entry_points.iter().map(|e| e.path.as_str()).collect();
                println!("{}    Entry points: {}", indent, eps.join(", "));
            }
            // Print rich Cargo metadata if available
            if let Some(ci) = &bp.cargo_info {
                if let Some(edition) = &ci.edition {
                    println!("{}    Edition: {}", indent, edition);
                }
                if !ci.authors.is_empty() {
                    println!("{}    Authors: {}", indent, ci.authors.join(", "));
                }
                if let Some(desc) = &ci.description {
                    let truncated: String = desc.chars().take(80).collect();
                    println!("{}    Description: {}", indent, truncated);
                }
                if let Some(license) = &ci.license {
                    println!("{}    License: {}", indent, license);
                }
                if let Some(rv) = &ci.rust_version {
                    println!("{}    MSRV: {}", indent, rv);
                }
                let normal_count = ci.dependencies.iter().filter(|d| d.kind == "normal").count();
                let dev_count = ci.dependencies.iter().filter(|d| d.kind == "dev").count();
                let build_count = ci.dependencies.iter().filter(|d| d.kind == "build").count();
                if normal_count > 0 || dev_count > 0 || build_count > 0 {
                    println!("{}    Dependencies: {} normal, {} dev, {} build", indent, normal_count, dev_count, build_count);
                }
                if !ci.features.is_empty() {
                    let feature_names: Vec<&str> = ci.features.keys().map(|k| k.as_str()).collect();
                    println!("{}    Features: {}", indent, feature_names.join(", "));
                }
                if !ci.targets.is_empty() {
                    let target_summaries: Vec<String> = ci.targets.iter().map(|t| {
                        let kind = t.kind.first().map(|k| k.as_str()).unwrap_or("unknown");
                        format!("{} ({})", t.name, kind)
                    }).collect();
                    println!("{}    Targets: {}", indent, target_summaries.join(", "));
                }
                if !ci.workspace_members.is_empty() {
                    let member_strs: Vec<String> = ci.workspace_members.iter().map(|m| {
                        format!("{} v{} ({})", m.name, m.version, m.path)
                    }).collect();
                    println!("{}    Workspace members: {}", indent, member_strs.join(", "));
                }
                if let Some(resolver) = &ci.workspace_resolver {
                    println!("{}    Workspace resolver: {}", indent, resolver);
                }
            }
            // Print sub-projects recursively
            for sub in &bp.sub_projects {
                print_build_project(sub, depth + 2);
            }
        }
        println!();
    }

    // ── Stage 3: Miscellaneous directories ─────────────────────────────────
    if !analysis.misc_directories.is_empty() {
        println!("{}--- Other Directories (Stage 3) ---", indent);
        for md in &analysis.misc_directories {
            let lang_names: Vec<&str> = md.languages.iter().map(|l| l.name.as_str()).collect();
            let lang_str = if lang_names.is_empty() {
                String::new()
            } else {
                format!(" [{}]", lang_names.join(", "))
            };
            println!("{}  {}/: {} ({} files{})", indent, md.path, md.role, md.file_count, lang_str);
        }
        println!();
    }

    println!("{}--- Summary ---", indent);
    println!("{}{}", indent, analysis.summary);
    println!();

    // Print sub-projects recursively (backward compat)
    for sub in &analysis.sub_projects {
        print_analysis(sub, depth + 1);
    }
}

/// Pretty-print a BuildProject (used for recursive sub-project display).
fn print_build_project(bp: &models::BuildProject, depth: usize) {
    let indent = "  ".repeat(depth);
    let name_str = bp.name.as_deref().unwrap_or("(unnamed)");
    let ver_str = bp.version.as_deref().map(|v| format!(" v{}", v)).unwrap_or_default();
    let ws_str = bp.is_workspace.map(|w| if w { " [workspace]" } else { "" }).unwrap_or("");
    println!(
        "{}  {}/{}: {}{}{} (conf: {:.0}%)",
        indent,
        bp.root,
        bp.build_file,
        name_str,
        ver_str,
        ws_str,
        bp.confidence * 100.0
    );
    if !bp.workspace_members.is_empty() {
        println!("{}    Members: {}", indent, bp.workspace_members.join(", "));
    }
    if !bp.languages.is_empty() {
        let lang_names: Vec<&str> = bp.languages.iter().map(|l| l.name.as_str()).collect();
        println!("{}    Languages: {}", indent, lang_names.join(", "));
    }
    if !bp.entry_points.is_empty() {
        let eps: Vec<&str> = bp.entry_points.iter().map(|e| e.path.as_str()).collect();
        println!("{}    Entry points: {}", indent, eps.join(", "));
    }
    // Print rich Cargo metadata if available
    if let Some(ci) = &bp.cargo_info {
        if let Some(edition) = &ci.edition {
            println!("{}    Edition: {}", indent, edition);
        }
        if !ci.authors.is_empty() {
            println!("{}    Authors: {}", indent, ci.authors.join(", "));
        }
        if let Some(desc) = &ci.description {
            let truncated: String = desc.chars().take(80).collect();
            println!("{}    Description: {}", indent, truncated);
        }
        if let Some(license) = &ci.license {
            println!("{}    License: {}", indent, license);
        }
        if let Some(rv) = &ci.rust_version {
            println!("{}    MSRV: {}", indent, rv);
        }
        // Dependencies summary
        let normal_count = ci.dependencies.iter().filter(|d| d.kind == "normal").count();
        let dev_count = ci.dependencies.iter().filter(|d| d.kind == "dev").count();
        let build_count = ci.dependencies.iter().filter(|d| d.kind == "build").count();
        if normal_count > 0 || dev_count > 0 || build_count > 0 {
            println!("{}    Dependencies: {} normal, {} dev, {} build", indent, normal_count, dev_count, build_count);
        }
        // Feature flags
        if !ci.features.is_empty() {
            let feature_names: Vec<&str> = ci.features.keys().map(|k| k.as_str()).collect();
            println!("{}    Features: {}", indent, feature_names.join(", "));
        }
        // Targets
        if !ci.targets.is_empty() {
            let target_summaries: Vec<String> = ci.targets.iter().map(|t| {
                let kind = t.kind.first().map(|k| k.as_str()).unwrap_or("unknown");
                format!("{} ({})", t.name, kind)
            }).collect();
            println!("{}    Targets: {}", indent, target_summaries.join(", "));
        }
        // Workspace members (from cargo_metadata)
        if !ci.workspace_members.is_empty() {
            let member_strs: Vec<String> = ci.workspace_members.iter().map(|m| {
                format!("{} v{} ({})", m.name, m.version, m.path)
            }).collect();
            println!("{}    Workspace members: {}", indent, member_strs.join(", "));
        }
        if let Some(resolver) = &ci.workspace_resolver {
            println!("{}    Workspace resolver: {}", indent, resolver);
        }
    }
    for sub in &bp.sub_projects {
        print_build_project(sub, depth + 1);
    }
}
