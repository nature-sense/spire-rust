//! Core git operations implementation using libgit2 (git2 crate).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::Path;

/// Result of a git operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitResult {
    pub success: bool,
    pub data: serde_json::Value,
    pub message: String,
}

impl GitResult {
    fn success(data: serde_json::Value, message: String) -> Self {
        Self {
            success: true,
            data,
            message,
        }
    }

    fn error(message: String) -> Self {
        Self {
            success: false,
            data: serde_json::Value::Null,
            message,
        }
    }
}

/// Git operations engine wrapping libgit2.
#[derive(Clone)]
pub struct GitOperations {
    repo_path: String,
}

impl Default for GitOperations {
    fn default() -> Self {
        Self::new(None)
    }
}

impl GitOperations {
    pub fn new(repo_path: Option<String>) -> Self {
        Self {
            repo_path: repo_path.unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            }),
        }
    }

    /// Open the git repository at the configured path.
    fn open_repo(&self) -> Result<git2::Repository, git2::Error> {
        // Resolve "." and relative paths to absolute
        let path = Path::new(&self.repo_path);
        let resolved = if path.to_string_lossy() == "." || path.is_relative() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| path.to_path_buf());
            cwd.join(path)
        } else {
            path.to_path_buf()
        };
        // Try canonicalizing, fall back to the resolved path
        let canonical = resolved.canonicalize().unwrap_or(resolved);
        git2::Repository::open(&canonical)
    }

    /// Execute a named git operation with the given arguments.
    pub async fn execute(
        &self,
        operation: &str,
        args: Option<HashMap<String, serde_json::Value>>,
    ) -> GitResult {
        let args = args.unwrap_or_default();

        let result = match operation {
            "status" => self.status(),
            "diff" => self.diff(&args),
            "log" => self.log(&args),
            "add" => self.add(&args),
            "commit" => self.commit(&args),
            "branch" => self.branch(&args),
            "checkout" => self.checkout(&args),
            "pull" => self.pull(&args),
            "push" => self.push(&args),
            _ => Err(git2::Error::from_str(&format!(
                "Unknown git operation: {}",
                operation
            ))),
        };

        match result {
            Ok(result) => result,
            Err(e) => GitResult::error(format!("Git operation failed: {}", e)),
        }
    }

    // ── status ────────────────────────────────────────────────────────────

    fn status(&self) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;
        let statuses = repo.statuses(None)?;

        let mut modified = Vec::new();
        let mut created = Vec::new();
        let mut deleted = Vec::new();
        let mut staged = Vec::new();
        let mut files = Vec::new();

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("unknown").to_string();
            let status = entry.status();

            if status == git2::Status::CURRENT {
                continue;
            }

            files.push(path.clone());

            if status.contains(git2::Status::INDEX_NEW)
                || status.contains(git2::Status::WT_NEW)
            {
                created.push(path.clone());
            }
            if status.contains(git2::Status::INDEX_MODIFIED)
                || status.contains(git2::Status::WT_MODIFIED)
            {
                modified.push(path.clone());
            }
            if status.contains(git2::Status::INDEX_DELETED)
                || status.contains(git2::Status::WT_DELETED)
            {
                deleted.push(path.clone());
            }
            if status != git2::Status::CURRENT
                && (status.contains(git2::Status::INDEX_NEW)
                    || status.contains(git2::Status::INDEX_MODIFIED)
                    || status.contains(git2::Status::INDEX_DELETED)
                    || status.contains(git2::Status::INDEX_TYPECHANGE)
                    || status.contains(git2::Status::INDEX_RENAMED))
            {
                staged.push(path);
            }
        }

        // Get current branch name
        let current = repo
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(String::from))
            .unwrap_or_else(|| "HEAD (detached)".to_string());

        let data = serde_json::json!({
            "modified": modified,
            "created": created,
            "deleted": deleted,
            "staged": staged,
            "files": files,
            "current": current,
        });

        Ok(GitResult::success(data, format!("On branch {}", current)))
    }

    // ── diff ──────────────────────────────────────────────────────────────

    fn diff(&self, args: &HashMap<String, serde_json::Value>) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;

        let files: Vec<String> = args
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let tree = repo.head()?.peel_to_tree()?;
        let diff = if files.is_empty() {
            repo.diff_tree_to_workdir(Some(&tree), None)?
        } else {
            let mut opts = git2::DiffOptions::new();
            for file in &files {
                opts.pathspec(file);
            }
            repo.diff_tree_to_workdir(Some(&tree), Some(&mut opts))?
        };

        let stats = diff.stats()?;
        let raw = diff_to_string(&diff);

        let diff_files: Vec<serde_json::Value> = diff
            .deltas()
            .map(|delta| {
                serde_json::json!({
                    "status": format!("{:?}", delta.status()),
                    "old_file": delta.old_file().path().map(|p| p.to_string_lossy()),
                    "new_file": delta.new_file().path().map(|p| p.to_string_lossy()),
                })
            })
            .collect();

        let data = serde_json::json!({
            "changed": stats.files_changed(),
            "insertions": stats.insertions(),
            "deletions": stats.deletions(),
            "files": diff_files,
            "raw": raw,
        });

        Ok(GitResult::success(
            data,
            format!(
                "Diff for {} — {} file(s) changed",
                if files.is_empty() {
                    "all files".to_string()
                } else {
                    files.join(", ")
                },
                stats.files_changed()
            ),
        ))
    }

    // ── log ───────────────────────────────────────────────────────────────

    fn log(&self, args: &HashMap<String, serde_json::Value>) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;
        let limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(10) as usize;
        let file_filter = args.get("file").and_then(|v| v.as_str()).map(String::from);

        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        let mut commits = Vec::new();
        let mut count = 0;

        for oid_result in revwalk {
            if count >= limit {
                break;
            }
            let oid = oid_result?;
            let commit = repo.find_commit(oid)?;

            // If file filter is set, check if this commit touches the file
            if let Some(ref file) = file_filter {
                let tree = commit.tree()?;
                let parent_tree = commit.parents().next().and_then(|p| p.tree().ok());
                let diff = repo.diff_tree_to_tree(
                    parent_tree.as_ref(),
                    Some(&tree),
                    Some(&mut {
                        let mut opts = git2::DiffOptions::new();
                        opts.pathspec(file);
                        opts
                    }),
                )?;
                if diff.deltas().len() == 0 {
                    continue;
                }
            }

            commits.push(serde_json::json!({
                "hash": oid.to_string(),
                "date": format_time(&commit.time()),
                "message": commit.message().unwrap_or("").trim(),
                "author": commit.author().name().unwrap_or("unknown"),
                "email": commit.author().email().unwrap_or("unknown"),
            }));
            count += 1;
        }

        let data = serde_json::json!({
            "commits": commits,
            "total": count,
        });

        Ok(GitResult::success(
            data,
            format!("Showing {} commits", count),
        ))
    }

    // ── add ───────────────────────────────────────────────────────────────

    fn add(&self, args: &HashMap<String, serde_json::Value>) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;
        let mut index = repo.index()?;

        let files: Vec<String> = args
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec![".".to_string()]);

        for file in &files {
            let path = if file == "." {
                // Use add_all for the entire working directory
                index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
                continue;
            } else {
                Path::new(file).to_path_buf()
            };
            index.add_path(&path)?;
        }
        index.write()?;

        Ok(GitResult::success(
            serde_json::json!({ "files": files }),
            format!("Added {}", files.join(", ")),
        ))
    }

    // ── commit ────────────────────────────────────────────────────────────

    fn commit(&self, args: &HashMap<String, serde_json::Value>) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;
        let message = args.get("message").and_then(|v| v.as_str()).ok_or_else(|| {
            git2::Error::from_str("Commit message is required")
        })?;

        let signature = repo.signature()?;
        let mut index = repo.index()?;
        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;

        // Handle initial commit (unborn branch) vs subsequent commits
        let parent_commit = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = parent_commit.iter().collect();

        let commit_oid = repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )?;

        Ok(GitResult::success(
            serde_json::json!({
                "commit": commit_oid.to_string(),
                "summary": message,
            }),
            format!("Committed: {}", message),
        ))
    }

    // ── branch ────────────────────────────────────────────────────────────

    fn branch(&self, args: &HashMap<String, serde_json::Value>) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;

        // If branch name is provided, create a new branch and switch to it
        if let Some(branch_name) = args.get("branch").and_then(|v| v.as_str()) {
            let head_commit = repo.head()?.peel_to_commit()?;
            repo.branch(branch_name, &head_commit, false)?;

            // Checkout the new branch
            let (object, reference) = repo.revparse_ext(branch_name)?;
            repo.checkout_tree(&object, None)?;
            match reference {
                Some(r) => repo.set_head(r.name().unwrap()),
                None => repo.set_head(format!("refs/heads/{}", branch_name).as_str()),
            }?;

            return Ok(GitResult::success(
                serde_json::json!({ "branch": branch_name }),
                format!("Created and switched to branch {}", branch_name),
            ));
        }

        // List branches
        let branches = repo.branches(None)?;
        let mut all = Vec::new();
        let mut current = String::new();

        for branch_result in branches {
            let (branch, _) = branch_result?;
            let name = branch.name()?.unwrap_or("unknown").to_string();
            let is_head = branch.is_head();
            if is_head {
                current = name.clone();
            }
            all.push(name);
        }

        let data = serde_json::json!({
            "current": current,
            "all": all,
        });

        Ok(GitResult::success(
            data,
            format!("Current branch: {}", current),
        ))
    }

    // ── checkout ──────────────────────────────────────────────────────────

    fn checkout(
        &self,
        args: &HashMap<String, serde_json::Value>,
    ) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;

        if let Some(branch) = args.get("branch").and_then(|v| v.as_str()) {
            let (object, reference) = repo.revparse_ext(branch)?;
            repo.checkout_tree(&object, None)?;
            match reference {
                Some(r) => repo.set_head(r.name().unwrap()),
                None => repo.set_head(format!("refs/heads/{}", branch).as_str()),
            }?;

            Ok(GitResult::success(
                serde_json::json!({ "branch": branch }),
                format!("Switched to branch {}", branch),
            ))
        } else if let Some(files) = args.get("files").and_then(|v| v.as_array()) {
            let paths: Vec<&Path> = files
                .iter()
                .filter_map(|v| v.as_str().map(Path::new))
                .collect();

            let mut checkout_opts = git2::build::CheckoutBuilder::new();
            for p in &paths {
                checkout_opts.path(p);
            }
            repo.checkout_head(Some(&mut checkout_opts))?;

            let file_list: Vec<String> =
                files.iter().filter_map(|v| v.as_str().map(String::from)).collect();

            Ok(GitResult::success(
                serde_json::json!({ "files": file_list }),
                format!("Restored {}", file_list.join(", ")),
            ))
        } else {
            Err(git2::Error::from_str(
                "Either branch or files are required for checkout",
            ))
        }
    }

    // ── pull ──────────────────────────────────────────────────────────────

    fn pull(&self, args: &HashMap<String, serde_json::Value>) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;
        let remote_name = args
            .get("remote")
            .and_then(|v| v.as_str())
            .unwrap_or("origin");
        let branch = args
            .get("branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main");

        let mut remote = repo.find_remote(remote_name)?;
        let refspecs = format!("refs/heads/{}:refs/heads/{}", branch, branch);

        // Fetch
        let mut fetch_opts = git2::FetchOptions::new();
        remote.fetch(&[&refspecs], Some(&mut fetch_opts), None)?;

        // Merge
        let fetch_head = repo.find_reference("FETCH_HEAD")?;
        let fetch_commit = fetch_head.peel_to_commit()?;
        let head_commit = repo.head()?.peel_to_commit()?;

        if head_commit.id() != fetch_commit.id() {
            let _signature = repo.signature()?;
            let tree = fetch_commit.tree()?;
            let head_tree = head_commit.tree()?;

            let merge_diff =
                repo.diff_tree_to_tree(Some(&head_tree), Some(&tree), None)?;
            if merge_diff.deltas().len() > 0 {
                let _merge_oid = repo.merge_commits(&head_commit, &fetch_commit, None)?;
                // Fast-forward
                repo.checkout_tree(fetch_commit.as_object(), None)?;
                let mut head = repo.head()?;
                head.set_target(fetch_commit.id(), "pull: fast-forward")?;
            }
        }

        Ok(GitResult::success(
            serde_json::json!({ "remote": remote_name, "branch": branch }),
            format!("Pulled from {}/{}", remote_name, branch),
        ))
    }

    // ── push ──────────────────────────────────────────────────────────────

    fn push(&self, args: &HashMap<String, serde_json::Value>) -> Result<GitResult, git2::Error> {
        let repo = self.open_repo()?;
        let remote_name = args
            .get("remote")
            .and_then(|v| v.as_str())
            .unwrap_or("origin");
        let branch = args
            .get("branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main");

        let mut remote = repo.find_remote(remote_name)?;
        let refspec = format!("refs/heads/{}:refs/heads/{}", branch, branch);

        let mut push_opts = git2::PushOptions::new();
        remote.push(&[&refspec], Some(&mut push_opts))?;

        Ok(GitResult::success(
            serde_json::json!({ "remote": remote_name, "branch": branch }),
            format!("Pushed to {}/{}", remote_name, branch),
        ))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Convert a diff to a string representation.
fn diff_to_string(diff: &git2::Diff) -> String {
    let mut output = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let content = std::str::from_utf8(line.content()).unwrap_or("");
        let prefix = match line.origin() {
            '+' => "+",
            '-' => "-",
            ' ' => " ",
            _ => " ",
        };
        let _ = write!(output, "{}{}", prefix, content);
        true
    })
    .ok();
    output
}

/// Format a git time to ISO-like string.
fn format_time(time: &git2::Time) -> String {
    let secs = time.seconds();
    // Use chrono for formatting if available, otherwise use a simple approach
    #[cfg(feature = "chrono")]
    {
        let naive = chrono::DateTime::from_timestamp(secs, 0)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "unknown".to_string());
        naive
    }
    #[cfg(not(feature = "chrono"))]
    {
        // Simple formatting without chrono
        let timestamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs.max(0) as u64);
        format!("{:?}", timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn init_repo() -> (TempDir, GitOperations) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Configure user for commits
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "test").unwrap();
        config.set_str("user.email", "test@test.com").unwrap();

        let ops = GitOperations::new(Some(dir.path().to_string_lossy().to_string()));
        (dir, ops)
    }

    fn create_file(dir: &TempDir, name: &str, content: &str) {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
    }

    fn stage_and_commit(ops: &GitOperations, msg: &str) {
        let mut args = HashMap::new();
        args.insert(
            "files".to_string(),
            serde_json::json!(["."]),
        );
        ops.add(&args).unwrap();

        let mut commit_args = HashMap::new();
        commit_args.insert(
            "message".to_string(),
            serde_json::json!(msg),
        );
        ops.commit(&commit_args).unwrap();
    }

    #[tokio::test]
    async fn test_status_empty_repo() {
        let (_dir, ops) = init_repo();
        let result = ops.execute("status", None).await;
        assert!(result.success, "Status should succeed: {}", result.message);
    }

    #[tokio::test]
    async fn test_status_with_changes() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");

        let result = ops.execute("status", None).await;
        assert!(result.success);
        assert_eq!(result.data["created"], serde_json::json!(["test.txt"]));
    }

    #[tokio::test]
    async fn test_add_and_commit() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");

        // Add
        let mut add_args = HashMap::new();
        add_args.insert(
            "files".to_string(),
            serde_json::json!(["test.txt"]),
        );
        let result = ops.execute("add", Some(add_args)).await;
        assert!(result.success, "Add failed: {}", result.message);

        // Commit
        let mut commit_args = HashMap::new();
        commit_args.insert(
            "message".to_string(),
            serde_json::json!("initial commit"),
        );
        let result = ops.execute("commit", Some(commit_args)).await;
        assert!(result.success, "Commit failed: {}", result.message);
        assert!(result.data["commit"].as_str().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_log() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");
        stage_and_commit(&ops, "first commit");

        create_file(&dir, "test.txt", "hello world");
        stage_and_commit(&ops, "second commit");

        let result = ops.execute("log", None).await;
        assert!(result.success);
        let commits = result.data["commits"].as_array().unwrap();
        assert_eq!(commits.len(), 2);
    }

    #[tokio::test]
    async fn test_branch_list() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");
        stage_and_commit(&ops, "initial");

        let result = ops.execute("branch", None).await;
        assert!(result.success);
        assert_eq!(result.data["current"], "master");
    }

    #[tokio::test]
    async fn test_branch_create() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");
        stage_and_commit(&ops, "initial");

        let mut args = HashMap::new();
        args.insert(
            "branch".to_string(),
            serde_json::json!("feature"),
        );
        let result = ops.execute("branch", Some(args)).await;
        assert!(result.success, "Branch create failed: {}", result.message);
        assert_eq!(result.data["branch"], "feature");
    }

    #[tokio::test]
    async fn test_checkout_branch() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");
        stage_and_commit(&ops, "initial");

        // Create branch first
        let mut branch_args = HashMap::new();
        branch_args.insert(
            "branch".to_string(),
            serde_json::json!("feature"),
        );
        ops.execute("branch", Some(branch_args)).await;

        // Checkout back to master
        let mut checkout_args = HashMap::new();
        checkout_args.insert(
            "branch".to_string(),
            serde_json::json!("master"),
        );
        let result = ops.execute("checkout", Some(checkout_args)).await;
        assert!(result.success, "Checkout failed: {}", result.message);
    }

    #[tokio::test]
    async fn test_diff() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");
        stage_and_commit(&ops, "initial");

        // Modify file
        create_file(&dir, "test.txt", "hello world");

        let result = ops.execute("diff", None).await;
        assert!(result.success);
        assert!(result.data["changed"].as_i64().unwrap_or(0) > 0);
    }

    #[tokio::test]
    async fn test_unknown_operation() {
        let (_dir, ops) = init_repo();
        let result = ops.execute("unknown_op", None).await;
        assert!(!result.success);
        assert!(result.message.contains("Unknown git operation"));
    }

    #[tokio::test]
    async fn test_commit_requires_message() {
        let (dir, ops) = init_repo();
        create_file(&dir, "test.txt", "hello");

        let mut args = HashMap::new();
        args.insert("files".to_string(), serde_json::json!(["."]));
        ops.execute("add", Some(args)).await;

        let result = ops.execute("commit", None).await;
        assert!(!result.success);
        assert!(result.message.contains("Commit message is required"));
    }
}
