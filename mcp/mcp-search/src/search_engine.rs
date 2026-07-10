//! Search engine for grep-like content search across files.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file: String,
    pub line: usize,
    pub content: String,
    pub context: SearchContext,
}

/// Context lines around a match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchContext {
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// Output of a search operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOutput {
    pub results: Vec<SearchResult>,
    pub total_matches: usize,
    pub search_time: u64,
}

/// Default glob patterns to exclude.
const DEFAULT_EXCLUDE: &[&str] = &[
    "**/node_modules/**",
    "**/.git/**",
    "**/dist/**",
    "**/build/**",
    "**/.next/**",
    "**/coverage/**",
    "**/.cache/**",
    "**/*.log",
    "**/*.min.js",
    "**/*.min.css",
    "**/vendor/**",
    "**/.DS_Store",
];

/// Search engine for file content searching.
pub struct SearchEngine;

impl SearchEngine {
    pub fn new() -> Self {
        Self
    }

    /// Perform a search across files.
    pub fn search(
        &self,
        pattern: &str,
        root_path: &str,
        is_regex: bool,
        case_sensitive: bool,
        context_lines: usize,
        max_results: usize,
        include: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
    ) -> SearchOutput {
        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Build the regex pattern
        let regex_pattern = if is_regex {
            pattern.to_string()
        } else {
            regex::escape(pattern)
        };

        let regex_flags = if case_sensitive { "" } else { "(?i)" };
        let full_pattern = format!("{}{}", regex_flags, regex_pattern);

        let re = match Regex::new(&full_pattern) {
            Ok(r) => r,
            Err(_e) => {
                return SearchOutput {
                    results: vec![],
                    total_matches: 0,
                    search_time: 0,
                };
            }
        };

        // Resolve files to search
        let files = self.resolve_files(root_path, include, exclude);

        // Search each file
        let mut all_results: Vec<SearchResult> = Vec::new();

        for file in &files {
            if all_results.len() >= max_results {
                break;
            }
            self.search_file(file, &re, context_lines, max_results, &mut all_results);
        }

        let end_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        SearchOutput {
            total_matches: all_results.len(),
            results: all_results,
            search_time: end_time - start_time,
        }
    }

    /// Resolve files to search based on root path and glob patterns.
    fn resolve_files(
        &self,
        root_path: &str,
        include: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
    ) -> Vec<String> {
        let root = Path::new(root_path);

        // If it's a single file, return it directly
        if root.is_file() {
            return vec![root_path.to_string()];
        }

        let exclude_patterns = exclude.unwrap_or_else(|| {
            DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect()
        });

        // Compile exclude globs
        let exclude_globs: Vec<globset::GlobMatcher> = exclude_patterns
            .iter()
            .filter_map(|p| globset::Glob::new(p).ok())
            .map(|g| g.compile_matcher())
            .collect();

        // Compile include globs
        let include_globs: Option<Vec<globset::GlobMatcher>> = include.map(|patterns| {
            patterns
                .iter()
                .filter_map(|p| globset::Glob::new(p).ok())
                .map(|g| g.compile_matcher())
                .collect()
        });

        let mut files = Vec::new();

        for entry in WalkDir::new(root_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                // Skip hidden directories
                let file_name = e.file_name().to_string_lossy();
                if e.depth() > 0 && file_name.starts_with('.') && e.file_type().is_dir() {
                    return false;
                }
                true
            })
        {
            if let Ok(entry) = entry {
                if !entry.file_type().is_file() {
                    continue;
                }

                let path = entry.path().to_string_lossy().to_string();

                // Check exclude patterns
                let is_excluded = exclude_globs.iter().any(|g| g.is_match(&path));
                if is_excluded {
                    continue;
                }

                // Check include patterns (if specified)
                if let Some(ref includes) = include_globs {
                    let is_included = includes.iter().any(|g| g.is_match(&path));
                    if !is_included {
                        continue;
                    }
                }

                files.push(path);
            }
        }

        files
    }

    /// Search a single file for matches.
    fn search_file(
        &self,
        file_path: &str,
        pattern: &Regex,
        context_lines: usize,
        max_results: usize,
        results: &mut Vec<SearchResult>,
    ) {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return, // Skip binary or unreadable files
        };

        // Check if file is likely binary
        if content.contains('\0') {
            return;
        }

        let lines: Vec<&str> = content.lines().collect();

        for (line_num, line) in lines.iter().enumerate() {
            if results.len() >= max_results {
                break;
            }

            if pattern.is_match(line) {
                // Before context
                let before_start = if line_num >= context_lines {
                    line_num - context_lines
                } else {
                    0
                };
                let before: Vec<String> = lines[before_start..line_num]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                // After context
                let after_end = std::cmp::min(line_num + 1 + context_lines, lines.len());
                let after: Vec<String> = lines[line_num + 1..after_end]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                results.push(SearchResult {
                    file: file_path.to_string(),
                    line: line_num + 1, // 1-based
                    content: line.to_string(),
                    context: SearchContext { before, after },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_files(dir: &TempDir) {
        // Create a test file
        fs::write(
            dir.path().join("test.txt"),
            "hello world\nthis is a test\nHELLO WORLD\nanother line\n",
        )
        .unwrap();

        // Create a subdirectory with more files
        fs::create_dir_all(dir.path().join("subdir")).unwrap();
        fs::write(
            dir.path().join("subdir/hello.rs"),
            "fn hello() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();

        fs::write(
            dir.path().join("subdir/world.js"),
            "const world = 'hello';\nconsole.log(world);\n",
        )
        .unwrap();
    }

    #[test]
    fn test_basic_plain_text_search() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "hello",
            dir.path().to_string_lossy().as_ref(),
            false,
            false,
            0,
            100,
            None,
            None,
        );

        assert!(result.total_matches > 0, "Should find matches");
        assert!(
            result.results.iter().any(|r| r.content.contains("hello")),
            "Should find 'hello' in results"
        );
    }

    #[test]
    fn test_case_sensitive_search() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "HELLO",
            dir.path().to_string_lossy().as_ref(),
            false,
            true,
            0,
            100,
            None,
            None,
        );

        assert!(result.total_matches > 0, "Should find HELLO");
        // All matches should contain uppercase HELLO
        for r in &result.results {
            assert!(
                r.content.contains("HELLO"),
                "Match should contain 'HELLO', got: {}",
                r.content
            );
        }
    }

    #[test]
    fn test_regex_search() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            r"h\w+o",
            dir.path().to_string_lossy().as_ref(),
            true,
            false,
            0,
            100,
            None,
            None,
        );

        assert!(result.total_matches > 0, "Should find regex matches");
    }

    #[test]
    fn test_context_lines() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "this is a test",
            dir.path().to_string_lossy().as_ref(),
            false,
            false,
            1,
            100,
            None,
            None,
        );

        assert!(result.total_matches > 0);
        // Should have before context
        assert!(
            result.results[0].context.before.len() > 0,
            "Should have before context"
        );
    }

    #[test]
    fn test_include_filter() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "hello",
            dir.path().to_string_lossy().as_ref(),
            false,
            false,
            0,
            100,
            Some(vec!["**/*.rs".to_string()]),
            None,
        );

        assert!(result.total_matches > 0);
        // All results should be .rs files
        for r in &result.results {
            assert!(
                r.file.ends_with(".rs"),
                "Result should be a .rs file, got: {}",
                r.file
            );
        }
    }

    #[test]
    fn test_exclude_filter() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "hello",
            dir.path().to_string_lossy().as_ref(),
            false,
            false,
            0,
            100,
            None,
            Some(vec!["**/*.txt".to_string()]),
        );

        // Should not find matches in .txt files
        for r in &result.results {
            assert!(
                !r.file.ends_with(".txt"),
                "Result should not be a .txt file, got: {}",
                r.file
            );
        }
    }

    #[test]
    fn test_single_file_search() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let file_path = dir.path().join("test.txt");

        let engine = SearchEngine::new();
        let result = engine.search(
            "hello",
            file_path.to_string_lossy().as_ref(),
            false,
            false,
            0,
            100,
            None,
            None,
        );

        assert!(result.total_matches > 0);
    }

    #[test]
    fn test_max_results() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "hello",
            dir.path().to_string_lossy().as_ref(),
            false,
            false,
            0,
            1,
            None,
            None,
        );

        assert!(
            result.total_matches <= 1,
            "Should have at most 1 result, got {}",
            result.total_matches
        );
    }

    #[test]
    fn test_no_matches() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "nonexistent_pattern_xyz",
            dir.path().to_string_lossy().as_ref(),
            false,
            false,
            0,
            100,
            None,
            None,
        );

        assert_eq!(result.total_matches, 0);
    }

    #[test]
    fn test_invalid_regex() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "[invalid",
            dir.path().to_string_lossy().as_ref(),
            true,
            false,
            0,
            100,
            None,
            None,
        );

        // Invalid regex should return empty results gracefully
        assert_eq!(result.total_matches, 0);
    }

    #[test]
    fn test_search_time_reported() {
        let dir = TempDir::new().unwrap();
        create_test_files(&dir);

        let engine = SearchEngine::new();
        let result = engine.search(
            "hello",
            dir.path().to_string_lossy().as_ref(),
            false,
            false,
            0,
            100,
            None,
            None,
        );

        assert!(
            result.search_time > 0,
            "Search time should be reported"
        );
    }
}
