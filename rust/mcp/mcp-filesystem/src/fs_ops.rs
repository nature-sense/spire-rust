//! Filesystem operations with path traversal protection.
//!
//! All operations validate that resolved paths stay within the allowed
//! directories to prevent path traversal attacks.

use anyhow::{bail, Context, Result};
use filetime::FileTime;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub permissions: String,
    pub modified: String,
    pub created: Option<String>,
    pub accessed: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DirectoryTreeEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<DirectoryTreeEntry>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Validates that a path is within one of the allowed directories.
/// Resolves symlinks and normalizes the path before checking.
pub fn validate_path(allowed_dirs: &[PathBuf], target: &Path) -> Result<PathBuf> {
    let canonical = target
        .canonicalize()
        .with_context(|| format!("Failed to resolve path: {}", target.display()))?;

    // Check that the canonical path starts with one of the allowed directories
    let allowed = allowed_dirs.iter().any(|dir| {
        let canonical_dir = if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            // If relative, resolve relative to current dir
            std::env::current_dir().unwrap_or_default().join(dir)
        };
        let canonical_dir = canonical_dir.canonicalize().unwrap_or(canonical_dir);
        canonical.starts_with(&canonical_dir)
    });

    if !allowed {
        bail!(
            "Path '{}' is not within allowed directories: {:?}",
            target.display(),
            allowed_dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
        );
    }

    Ok(canonical)
}

/// Validate that a parent directory for a new file is within allowed dirs.
/// Unlike `validate_path`, the target path may not exist yet, so we check
/// the parent chain.
pub fn validate_parent_path(allowed_dirs: &[PathBuf], target: &Path) -> Result<PathBuf> {
    // Get the absolute path (resolve relative to cwd)
    let abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(target)
    };

    // Normalize the path (remove .. and . components)
    let normalized = normalize_path(&abs);

    // Walk up the path to find the first existing ancestor, then canonicalize
    let mut check = normalized.as_path();
    loop {
        if check.exists() {
            // Validate this existing ancestor is within allowed dirs
            validate_path(allowed_dirs, check)?;
            break;
        }
        match check.parent() {
            Some(parent) => check = parent,
            None => {
                bail!(
                    "Path '{}' has no existing ancestor within allowed directories",
                    target.display()
                );
            }
        }
    }

    Ok(normalized)
}

/// Normalize a path by removing `.` and `..` components.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(_) => components.push(component),
            Component::CurDir => {} // skip "."
            Component::ParentDir => {
                components.pop(); // pop last for ".."
            }
            c => components.push(c),
        }
    }
    components.iter().collect()
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Read the contents of a file.
pub fn read_file(allowed_dirs: &[PathBuf], path: &str) -> Result<String> {
    let target = Path::new(path);
    let canonical = validate_path(allowed_dirs, target)?;

    let content = fs::read_to_string(&canonical)
        .with_context(|| format!("Failed to read file: {}", canonical.display()))?;

    Ok(content)
}

/// Read a portion of a file (offset + limit in bytes).
pub fn read_file_range(
    allowed_dirs: &[PathBuf],
    path: &str,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<String> {
    let target = Path::new(path);
    let canonical = validate_path(allowed_dirs, target)?;

    let file = fs::File::open(&canonical)
        .with_context(|| format!("Failed to open file: {}", canonical.display()))?;

    let mut reader = BufReader::new(file);

    if let Some(off) = offset {
        let mut buf = reader.fill_buf().context("Failed to read file buffer")?;
        if off as usize > buf.len() {
            bail!(
                "Offset {} exceeds file size {}",
                off,
                buf.len()
            );
        }
        buf.consume(off as usize);
    }

    let mut content = String::new();
    if let Some(lim) = limit {
        let mut handle = reader.take(lim);
        handle
            .read_to_string(&mut content)
            .context("Failed to read file range")?;
    } else {
        reader
            .read_to_string(&mut content)
            .context("Failed to read file")?;
    }

    Ok(content)
}

/// Read multiple files at once.
pub fn read_multiple_files(allowed_dirs: &[PathBuf], paths: &[String]) -> Result<Vec<FileReadResult>> {
    let mut results = Vec::new();
    for path in paths {
        match read_file(allowed_dirs, path) {
            Ok(content) => results.push(FileReadResult {
                path: path.clone(),
                content: Some(content),
                error: None,
            }),
            Err(e) => results.push(FileReadResult {
                path: path.clone(),
                content: None,
                error: Some(e.to_string()),
            }),
        }
    }
    Ok(results)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileReadResult {
    pub path: String,
    pub content: Option<String>,
    pub error: Option<String>,
}

/// Write content to a file (create or overwrite).
pub fn write_file(allowed_dirs: &[PathBuf], path: &str, content: &str) -> Result<()> {
    let target = Path::new(path);
    let normalized = validate_parent_path(allowed_dirs, target)?;

    // Ensure parent directory exists
    if let Some(parent) = normalized.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directories: {}", parent.display()))?;
    }

    fs::write(&normalized, content)
        .with_context(|| format!("Failed to write file: {}", normalized.display()))?;

    Ok(())
}

/// Edit a file by performing a find/replace operation.
pub fn edit_file(
    allowed_dirs: &[PathBuf],
    path: &str,
    old_string: &str,
    new_string: &str,
) -> Result<EditResult> {
    let target = Path::new(path);
    let canonical = validate_path(allowed_dirs, target)?;

    let content = fs::read_to_string(&canonical)
        .with_context(|| format!("Failed to read file: {}", canonical.display()))?;

    if !content.contains(old_string) {
        bail!(
            "String not found in file '{}': '{}'",
            canonical.display(),
            old_string
        );
    }

    let new_content = content.replace(old_string, new_string);
    let diff_count = content.matches(old_string).count();

    fs::write(&canonical, &new_content)
        .with_context(|| format!("Failed to write file: {}", canonical.display()))?;

    Ok(EditResult {
        path: canonical.display().to_string(),
        replacements: diff_count,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EditResult {
    pub path: String,
    pub replacements: usize,
}

/// Create a directory (with parents, like `mkdir -p`).
pub fn create_directory(allowed_dirs: &[PathBuf], path: &str) -> Result<()> {
    let target = Path::new(path);
    let normalized = validate_parent_path(allowed_dirs, target)?;

    fs::create_dir_all(&normalized)
        .with_context(|| format!("Failed to create directory: {}", normalized.display()))?;

    Ok(())
}

/// List files and directories in a path.
pub fn list_directory(
    allowed_dirs: &[PathBuf],
    path: &str,
    recursive: bool,
) -> Result<Vec<DirectoryEntry>> {
    let target = Path::new(path);
    let canonical = validate_path(allowed_dirs, target)?;

    if !canonical.is_dir() {
        bail!("Path is not a directory: {}", canonical.display());
    }

    let mut entries = Vec::new();

    if recursive {
        for entry in WalkDir::new(&canonical).follow_links(false) {
            let entry = entry.with_context(|| {
                format!("Failed to walk directory: {}", canonical.display())
            })?;
            let metadata = entry.metadata().with_context(|| {
                format!("Failed to get metadata for: {}", entry.path().display())
            })?;
            let file_type = entry.file_type();
            entries.push(DirectoryEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path().display().to_string(),
                is_directory: file_type.is_dir(),
                is_symlink: file_type.is_symlink(),
                size: metadata.len(),
            });
        }
    } else {
        let dir = fs::read_dir(&canonical).with_context(|| {
            format!("Failed to read directory: {}", canonical.display())
        })?;

        for entry in dir {
            let entry = entry.with_context(|| {
                format!("Failed to read directory entry in: {}", canonical.display())
            })?;
            let metadata = entry.metadata().with_context(|| {
                format!("Failed to get metadata for: {}", entry.path().display())
            })?;
            let file_type = entry.file_type().with_context(|| {
                format!("Failed to get file type for: {}", entry.path().display())
            })?;
            entries.push(DirectoryEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path().display().to_string(),
                is_directory: file_type.is_dir(),
                is_symlink: file_type.is_symlink(),
                size: metadata.len(),
            });
        }
    }

    // Sort by name for consistent output
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(entries)
}

/// Get a recursive directory tree structure.
pub fn directory_tree(allowed_dirs: &[PathBuf], path: &str) -> Result<DirectoryTreeEntry> {
    let target = Path::new(path);
    let canonical = validate_path(allowed_dirs, target)?;

    build_tree(&canonical, &canonical)
}

fn build_tree(root: &Path, current: &Path) -> Result<DirectoryTreeEntry> {
    let metadata = current.metadata().with_context(|| {
        format!("Failed to get metadata for: {}", current.display())
    })?;

    let mut entry = DirectoryTreeEntry {
        name: current
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| current.display().to_string()),
        path: current.display().to_string(),
        is_directory: current.is_dir(),
        is_symlink: current.is_symlink(),
        size: metadata.len(),
        children: None,
    };

    if current.is_dir() {
        let mut children = Vec::new();
        let dir = fs::read_dir(current).with_context(|| {
            format!("Failed to read directory: {}", current.display())
        })?;

        for child in dir {
            let child = child.with_context(|| {
                format!("Failed to read entry in: {}", current.display())
            })?;
            let child_path = child.path();

            // Skip if outside root (shouldn't happen with normal dirs)
            if !child_path.starts_with(root) {
                continue;
            }

            match build_tree(root, &child_path) {
                Ok(child_entry) => children.push(child_entry),
                Err(_e) => {
                    // Include error info as a leaf entry
                    children.push(DirectoryTreeEntry {
                        name: child_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        path: child_path.display().to_string(),
                        is_directory: false,
                        is_symlink: false,
                        size: 0,
                        children: None,
                    });
                }
            }
        }

        // Sort children by name
        children.sort_by(|a, b| a.path.cmp(&b.path));
        entry.children = Some(children);
    }

    Ok(entry)
}

/// Move/rename a file or directory.
pub fn move_file(
    allowed_dirs: &[PathBuf],
    source: &str,
    destination: &str,
) -> Result<()> {
    let src = Path::new(source);
    let dst = Path::new(destination);

    let canonical_src = validate_path(allowed_dirs, src)?;
    let canonical_dst = validate_parent_path(allowed_dirs, dst)?;

    // Ensure parent of destination exists
    if let Some(parent) = canonical_dst.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create parent directories: {}", parent.display())
        })?;
    }

    fs::rename(&canonical_src, &canonical_dst).with_context(|| {
        format!(
            "Failed to move '{}' to '{}'",
            canonical_src.display(),
            canonical_dst.display()
        )
    })?;

    Ok(())
}

/// Copy a file or directory.
pub fn copy_file(
    allowed_dirs: &[PathBuf],
    source: &str,
    destination: &str,
    recursive: bool,
) -> Result<()> {
    let src = Path::new(source);
    let dst = Path::new(destination);

    let canonical_src = validate_path(allowed_dirs, src)?;
    let canonical_dst = validate_parent_path(allowed_dirs, dst)?;

    if canonical_src.is_dir() {
        if !recursive {
            bail!("Source is a directory; use recursive=true to copy directories");
        }
        copy_dir_all(&canonical_src, &canonical_dst)?;
    } else {
        // Ensure parent of destination exists
        if let Some(parent) = canonical_dst.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directories: {}", parent.display())
            })?;
        }
        fs::copy(&canonical_src, &canonical_dst).with_context(|| {
            format!(
                "Failed to copy '{}' to '{}'",
                canonical_src.display(),
                canonical_dst.display()
            )
        })?;
    }

    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| {
        format!("Failed to create directory: {}", dst.display())
    })?;

    for entry in fs::read_dir(src).with_context(|| {
        format!("Failed to read directory: {}", src.display())
    })? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(&entry.path(), &target).with_context(|| {
                format!(
                    "Failed to copy '{}' to '{}'",
                    entry.path().display(),
                    target.display()
                )
            })?;
        }
    }

    Ok(())
}

/// Delete a file or directory.
pub fn delete_file(allowed_dirs: &[PathBuf], path: &str, recursive: bool) -> Result<()> {
    let target = Path::new(path);
    let canonical = validate_path(allowed_dirs, target)?;

    if canonical.is_dir() {
        if !recursive {
            bail!(
                "Path is a directory: {}; use recursive=true to delete directories",
                canonical.display()
            );
        }
        fs::remove_dir_all(&canonical).with_context(|| {
            format!("Failed to remove directory: {}", canonical.display())
        })?;
    } else {
        fs::remove_file(&canonical).with_context(|| {
            format!("Failed to remove file: {}", canonical.display())
        })?;
    }

    Ok(())
}

/// Get file metadata.
pub fn get_file_info(allowed_dirs: &[PathBuf], path: &str) -> Result<FileInfo> {
    let target = Path::new(path);
    let canonical = validate_path(allowed_dirs, target)?;

    let metadata = canonical.metadata().with_context(|| {
        format!("Failed to get metadata for: {}", canonical.display())
    })?;

    let modified = FileTime::from_last_modification_time(&metadata);
    let accessed = FileTime::from_last_access_time(&metadata);
    let created = metadata.created().ok().map(FileTime::from_system_time);

    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        format!("{:o}", metadata.permissions().mode())
    };
    #[cfg(not(unix))]
    let permissions = format!("{:?}", metadata.permissions());

    Ok(FileInfo {
        path: canonical.display().to_string(),
        size: metadata.len(),
        is_directory: metadata.is_dir(),
        is_symlink: canonical.is_symlink(),
        permissions,
        modified: format!(
            "{}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            modified.seconds() / 86400 / 365 + 1970, // rough year
            (modified.seconds() / 86400 % 365) / 30 + 1, // rough month
            modified.seconds() / 86400 % 30 + 1, // rough day
            modified.seconds() / 3600 % 24,
            modified.seconds() / 60 % 60,
            modified.seconds() % 60,
        ),
        created: created.map(|c| {
            format!(
                "{}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                c.seconds() / 86400 / 365 + 1970,
                (c.seconds() / 86400 % 365) / 30 + 1,
                c.seconds() / 86400 % 30 + 1,
                c.seconds() / 3600 % 24,
                c.seconds() / 60 % 60,
                c.seconds() % 60,
            )
        }),
        accessed: Some(format!(
            "{}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            accessed.seconds() / 86400 / 365 + 1970,
            (accessed.seconds() / 86400 % 365) / 30 + 1,
            accessed.seconds() / 86400 % 30 + 1,
            accessed.seconds() / 3600 % 24,
            accessed.seconds() / 60 % 60,
            accessed.seconds() % 60,
        )),
    })
}

/// Search for files by glob pattern.
pub fn search_files(
    allowed_dirs: &[PathBuf],
    pattern: &str,
    root_path: Option<&str>,
) -> Result<Vec<SearchResult>> {
    let root = if let Some(rp) = root_path {
        let p = Path::new(rp);
        validate_path(allowed_dirs, p)?
    } else {
        // Use the first allowed directory as root
        allowed_dirs
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("."))
    };

    // Build glob set
    let mut builder = GlobSetBuilder::new();
    builder
        .add(Glob::new(pattern).with_context(|| format!("Invalid glob pattern: {}", pattern))?);
    let glob_set: GlobSet = builder
        .build()
        .context("Failed to build glob set")?;

    let mut results = Vec::new();

    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let relative = entry
            .path()
            .strip_prefix(&root)
            .unwrap_or(entry.path());

        if glob_set.is_match(relative) {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            results.push(SearchResult {
                path: entry.path().display().to_string(),
                name: entry.file_name().to_string_lossy().to_string(),
                is_directory: entry.file_type().is_dir(),
                size: metadata.len(),
            });
        }
    }

    // Sort by path
    results.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(results)
}

/// Check if a file or directory exists.
pub fn file_exists(allowed_dirs: &[PathBuf], path: &str) -> Result<bool> {
    let target = Path::new(path);

    // For existence checks, we try to validate but if the path doesn't exist
    // we still need to check if it *would* be allowed
    if target.exists() {
        validate_path(allowed_dirs, target)?;
        Ok(true)
    } else {
        // Check if the parent is within allowed dirs
        if let Some(parent) = target.parent() {
            if parent.exists() {
                validate_path(allowed_dirs, parent)?;
            }
        }
        Ok(false)
    }
}

/// Get the list of allowed directories.
pub fn get_allowed_directories(allowed_dirs: &[PathBuf]) -> Vec<String> {
    allowed_dirs
        .iter()
        .map(|d| {
            if d.is_absolute() {
                d.display().to_string()
            } else {
                std::env::current_dir()
                    .unwrap_or_default()
                    .join(d)
                    .display()
                    .to_string()
            }
        })
        .collect()
}
