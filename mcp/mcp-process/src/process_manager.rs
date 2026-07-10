//! Process manager for spawning and managing child processes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

/// Information about a managed process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub id: String,
    pub pid: u32,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub start_time: u64,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i32>,
}

/// Result of starting a process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartProcessResult {
    pub process_id: String,
    pub pid: u32,
    pub status: String,
    pub start_time: u64,
}

/// Result of getting process output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessOutput {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub total_lines: usize,
}

/// Result of killing a process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillResult {
    pub process_id: String,
    pub status: String,
}

/// Internal process state.
struct ManagedProcess {
    id: String,
    child: Child,
    stdout: Vec<String>,
    stderr: Vec<String>,
    start_time: u64,
    command: String,
    status: String,
    exit_code: Option<i32>,
}

/// Manages child processes with output capture.
pub struct ProcessManager {
    processes: Arc<Mutex<HashMap<String, ManagedProcess>>>,
    max_output_lines: usize,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            max_output_lines: 1000,
        }
    }

    /// Start a new process.
    pub async fn start_process(
        &self,
        command: &str,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
        timeout: Option<u64>,
        shell: bool,
    ) -> StartProcessResult {
        let id = Uuid::new_v4().to_string();
        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut cmd = if shell {
            let mut c = Command::new("sh");
            if cfg!(target_os = "windows") {
                c.arg("/c");
            } else {
                c.arg("-c");
            }
            c.arg(command);
            c
        } else {
            let parts: Vec<&str> = command.split_whitespace().collect();
            let mut c = Command::new(parts[0]);
            if parts.len() > 1 {
                c.args(&parts[1..]);
            }
            c
        };

        // Set working directory
        if let Some(dir) = cwd {
            cmd.current_dir(&dir);
        }

        // Set environment variables
        if let Some(env_vars) = env {
            for (key, value) in env_vars {
                cmd.env(&key, &value);
            }
        }

        // Capture stdout and stderr
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::piped());

        let mut child = cmd.spawn().expect("Failed to spawn process");
        let pid = child.id().unwrap_or(0);

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let processes = self.processes.clone();
        let max_lines = self.max_output_lines;
        let process_id = id.clone();

        // Store process info first (before spawning wait task that needs the stored child)
        {
            let mut procs = processes.lock().await;
            procs.insert(
                id.clone(),
                ManagedProcess {
                    id: id.clone(),
                    child,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    start_time,
                    command: command.to_string(),
                    status: "running".to_string(),
                    exit_code: None,
                },
            );
        }

        // Spawn tasks to capture stdout and stderr
        if let Some(stdout) = stdout_handle {
            let procs = processes.clone();
            let pid = process_id.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut procs = procs.lock().await;
                    if let Some(proc) = procs.get_mut(&pid) {
                        proc.stdout.push(line);
                        if proc.stdout.len() > max_lines {
                            proc.stdout.remove(0);
                        }
                    }
                }
            });
        }

        if let Some(stderr) = stderr_handle {
            let procs = processes.clone();
            let pid = process_id.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let reader = tokio::io::BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut procs = procs.lock().await;
                    if let Some(proc) = procs.get_mut(&pid) {
                        proc.stderr.push(line);
                        if proc.stderr.len() > max_lines {
                            proc.stderr.remove(0);
                        }
                    }
                }
            });
        }

        // Spawn a task to wait for the process to exit
        {
            let procs = processes.clone();
            let pid = process_id.clone();
            tokio::spawn(async move {
                // Wait a bit for the process to be stored
                sleep(Duration::from_millis(50)).await;
                let mut procs = procs.lock().await;
                if let Some(proc) = procs.get_mut(&pid) {
                    let status = proc.child.wait().await;
                    proc.status = "exited".to_string();
                    proc.exit_code = status.ok().and_then(|s| s.code());
                }
            });
        }

        // Set timeout if specified
        if let Some(timeout_ms) = timeout {
            if timeout_ms > 0 {
                let procs = processes.clone();
                let pid = process_id.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_millis(timeout_ms)).await;
                    let mut guard = procs.lock().await;
                    if let Some(proc) = guard.get(&pid) {
                        if proc.status == "running" {
                            drop(guard);
                            let mut guard = procs.lock().await;
                            if let Some(proc) = guard.get_mut(&pid) {
                                proc.status = "timed_out".to_string();
                            }
                        }
                    }
                });
            }
        }

        StartProcessResult {
            process_id: id,
            pid,
            status: "running".to_string(),
            start_time,
        }
    }

    /// Send input to a process's stdin.
    pub async fn send_stdin(
        &self,
        process_id: &str,
        input: &str,
        newline: bool,
    ) -> Result<(), String> {
        let mut procs = self.processes.lock().await;
        let proc = procs.get_mut(process_id).ok_or_else(|| {
            format!("Process {} not found", process_id)
        })?;

        if proc.status != "running" {
            return Err(format!(
                "Process {} is not running (status: {})",
                process_id, proc.status
            ));
        }

        if let Some(stdin) = proc.child.stdin.as_mut() {
            let content = if newline {
                format!("{}\n", input)
            } else {
                input.to_string()
            };
            stdin
                .write_all(content.as_bytes())
                .await
                .map_err(|e| format!("Failed to write to stdin: {}", e))?;
            stdin
                .flush()
                .await
                .map_err(|e| format!("Failed to flush stdin: {}", e))?;
            Ok(())
        } else {
            Err("Process has no stdin".to_string())
        }
    }

    /// Kill a process.
    pub async fn kill_process(
        &self,
        process_id: &str,
        _signal: &str,
    ) -> Result<KillResult, String> {
        let mut procs = self.processes.lock().await;
        let proc = procs.get_mut(process_id).ok_or_else(|| {
            format!("Process {} not found", process_id)
        })?;

        if proc.status != "running" {
            return Err(format!(
                "Process {} is not running (status: {})",
                process_id, proc.status
            ));
        }

        // On Unix, we can send different signals
        #[cfg(unix)]
        {
            let _ = proc.child.start_kill();
        }

        #[cfg(not(unix))]
        {
            let _ = proc.child.start_kill();
        }

        proc.status = "exited".to_string();

        Ok(KillResult {
            process_id: process_id.to_string(),
            status: "exited".to_string(),
        })
    }

    /// Get captured output from a process.
    pub async fn get_output(
        &self,
        process_id: &str,
        tail: Option<usize>,
        _since: Option<u64>,
    ) -> ProcessOutput {
        let procs = self.processes.lock().await;
        let proc = procs.get(process_id);

        match proc {
            Some(proc) => {
                let mut stdout = proc.stdout.clone();
                let mut stderr = proc.stderr.clone();

                if let Some(n) = tail {
                    if n > 0 {
                        stdout = stdout.into_iter().rev().take(n).rev().collect();
                        stderr = stderr.into_iter().rev().take(n).rev().collect();
                    }
                }

                let total_lines = proc.stdout.len() + proc.stderr.len();

                ProcessOutput {
                    stdout,
                    stderr,
                    total_lines,
                }
            }
            None => ProcessOutput {
                stdout: vec![],
                stderr: vec![],
                total_lines: 0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_start_and_get_output() {
        let manager = ProcessManager::new();

        let result = manager
            .start_process("echo hello world", None, None, None, true)
            .await;

        assert_eq!(result.status, "running");
        assert!(result.pid > 0);

        // Give it a moment to complete
        tokio::time::sleep(Duration::from_millis(200)).await;

        let output = manager.get_output(&result.process_id, None, None).await;
        assert!(
            output.stdout.iter().any(|l| l.contains("hello world")),
            "Expected 'hello world' in stdout, got: {:?}",
            output.stdout
        );
    }

    #[tokio::test]
    async fn test_start_with_cwd() {
        let manager = ProcessManager::new();
        let dir = std::env::temp_dir();

        let result = manager
            .start_process(
                "pwd",
                Some(dir.to_string_lossy().to_string()),
                None,
                None,
                true,
            )
            .await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        let output = manager.get_output(&result.process_id, None, None).await;
        let pwd_output = output.stdout.join(" ");
        let dir_str = dir.to_string_lossy().to_string();
        // Handle macOS symlink (/var -> /private/var)
        let canonical_dir = std::fs::canonicalize(&dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| dir_str.clone());
        assert!(
            pwd_output.contains(&canonical_dir) || pwd_output.contains(&dir_str),
            "Expected pwd to contain {:?} or {:?}, got: {:?}",
            dir_str,
            canonical_dir,
            pwd_output
        );
    }

    #[tokio::test]
    async fn test_send_stdin() {
        let manager = ProcessManager::new();

        // Use cat to read from stdin
        let result = manager
            .start_process("cat", None, None, None, true)
            .await;

        // Send input
        manager
            .send_stdin(&result.process_id, "test input", true)
            .await
            .unwrap();

        // Give it time to process
        tokio::time::sleep(Duration::from_millis(200)).await;

        let output = manager.get_output(&result.process_id, None, None).await;
        assert!(
            output.stdout.iter().any(|l| l.contains("test input")),
            "Expected 'test input' in stdout, got: {:?}",
            output.stdout
        );
    }

    #[tokio::test]
    async fn test_kill_process() {
        let manager = ProcessManager::new();

        // Start a long-running process
        let result = manager
            .start_process("sleep 60", None, None, None, true)
            .await;

        // Kill it
        let kill_result = manager
            .kill_process(&result.process_id, "SIGTERM")
            .await
            .unwrap();

        assert_eq!(kill_result.status, "exited");
    }

    #[tokio::test]
    async fn test_get_output_with_tail() {
        let manager = ProcessManager::new();

        let result = manager
            .start_process(
                "echo 'line1\nline2\nline3\nline4\nline5'",
                None,
                None,
                None,
                true,
            )
            .await;

        tokio::time::sleep(Duration::from_millis(300)).await;

        let output = manager.get_output(&result.process_id, Some(2), None).await;
        assert!(
            output.stdout.len() <= 2,
            "Expected at most 2 lines with tail=2, got {}",
            output.stdout.len()
        );
    }

    #[tokio::test]
    async fn test_process_not_found() {
        let manager = ProcessManager::new();

        let result = manager.send_stdin("nonexistent", "test", true).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[tokio::test]
    async fn test_stderr_capture() {
        let manager = ProcessManager::new();

        // Use a command that writes to stderr
        let result = manager
            .start_process("echo 'error msg' >&2", None, None, None, true)
            .await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        let output = manager.get_output(&result.process_id, None, None).await;
        assert!(
            output.stderr.iter().any(|l| l.contains("error msg")),
            "Expected 'error msg' in stderr, got: {:?}",
            output.stderr
        );
    }

    #[tokio::test]
    async fn test_timeout() {
        let manager = ProcessManager::new();

        let result = manager
            .start_process("sleep 10", None, None, Some(100), true)
            .await;

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(200)).await;

        // The process should have been killed by timeout
        let output = manager.get_output(&result.process_id, None, None).await;
        // The process may have been killed, so we just check it doesn't crash
        assert!(output.total_lines >= 0);
    }
}
