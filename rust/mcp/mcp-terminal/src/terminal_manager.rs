//! Terminal manager — PTY-based interactive terminal sessions.
//!
//! Each session spawns a shell (e.g. /bin/zsh) connected to a POSIX PTY.
//! The master side is kept as an OwnedFd; reads/writes use nix::unistd
//! (non-blocking) so we avoid the IO Safety issues of wrapping/unwrapping
//! the same fd in tokio File handles.

use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::pty;
use nix::unistd::{close, dup2, read, setsid, write, Pid};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Information about a terminal session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub shell: String,
    pub pid: u32,
    pub cols: u16,
    pub rows: u16,
    pub created_at: u64,
    pub status: String,
    pub output_lines: usize,
}

/// Result of spawning a terminal session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnResult {
    pub session_id: String,
    pub pid: u32,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    pub created_at: u64,
}

/// Result of reading from a terminal session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResult {
    pub session_id: String,
    pub data: String,
    pub bytes_read: usize,
    pub eof: bool,
}

/// Result of listing sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResult {
    pub sessions: Vec<SessionInfo>,
}

/// Internal session state.
struct TerminalSession {
    id: String,
    shell: String,
    pid: u32,
    master_fd: OwnedFd,
    cols: u16,
    rows: u16,
    created_at: u64,
    status: String,
    /// Ring buffer of output lines
    output: Vec<String>,
    max_output_lines: usize,
}

impl TerminalSession {
    /// Write data to the PTY master (feeds into shell's stdin).
    async fn write(&mut self, data: &str) -> Result<(), String> {
        if self.status != "running" {
            return Err(format!(
                "Session {} is not running (status: {})",
                self.id, self.status
            ));
        }
        let bytes = data.as_bytes();
        let mut written = 0;
        let fd = self.master_fd.as_raw_fd();
        while written < bytes.len() {
            match write(unsafe { BorrowedFd::borrow_raw(fd) }, &bytes[written..]) {
                Ok(n) => written += n,
                Err(nix::errno::Errno::EAGAIN) => {
                    // PTY buffer full, yield and retry
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    continue;
                }
                Err(e) => return Err(format!("write error: {}", e)),
            }
        }
        Ok(())
    }

    /// Read pending data from the PTY master (non-blocking).
    async fn read(&mut self, max_bytes: usize) -> Result<ReadResult, String> {
        if self.status != "running" {
            return Err(format!(
                "Session {} is not running (status: {})",
                self.id, self.status
            ));
        }

        let mut buf = vec![0u8; max_bytes];
        let fd = self.master_fd.as_raw_fd();
        let n = match read(fd, &mut buf) {
            Ok(n) => n,
            Err(nix::errno::Errno::EAGAIN) => 0,
            Err(e) => return Err(format!("read error: {}", e)),
        };

        let data = String::from_utf8_lossy(&buf[..n]).to_string();

        // Store in ring buffer
        for line in data.lines() {
            self.output.push(line.to_string());
            if self.output.len() > self.max_output_lines {
                self.output.remove(0);
            }
        }

        Ok(ReadResult {
            session_id: self.id.clone(),
            data,
            bytes_read: n,
            eof: n == 0,
        })
    }

    /// Resize the terminal.
    fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        let ws = nix::libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // On macOS, TIOCSWINSZ must be called on the slave side of the PTY.
        // We open the slave via ptsname on the master fd.
        let slave_name = unsafe {
            let name = nix::libc::ptsname(self.master_fd.as_raw_fd());
            if name.is_null() {
                return Err(format!(
                    "ptsname failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            std::ffi::CStr::from_ptr(name).to_string_lossy().to_string()
        };
        let slave_fd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&slave_name)
            .map_err(|e| format!("failed to open slave PTY for resize: {}", e))?;
        let rc = unsafe { nix::libc::ioctl(slave_fd.as_raw_fd(), nix::libc::TIOCSWINSZ, &ws) };
        drop(slave_fd);
        if rc != 0 {
            return Err(format!(
                "ioctl(TIOCSWINSZ) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        self.cols = cols;
        self.rows = rows;
        Ok(())
    }
}

/// Manages PTY-based terminal sessions.
pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
    max_output_lines: usize,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            max_output_lines: 10000,
        }
    }

    /// Spawn a new terminal session with a PTY.
    ///
    /// This forks a child process, sets up the PTY slave as its controlling
    /// terminal, and execs the shell. The master fd is kept for I/O.
    pub async fn spawn(
        &self,
        shell: Option<String>,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
        cols: Option<u16>,
        rows: Option<u16>,
    ) -> Result<SpawnResult, String> {
        let shell = shell.unwrap_or_else(|| "/bin/zsh".to_string());
        let cols = cols.unwrap_or(80);
        let rows = rows.unwrap_or(24);
        let id = Uuid::new_v4().to_string();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Allocate a PTY
        let master_fd = pty::posix_openpt(OFlag::O_RDWR | OFlag::O_NONBLOCK)
            .map_err(|e| format!("posix_openpt failed: {}", e))?;
        let master_raw = master_fd.as_raw_fd();

        // Grant access and unlock the slave
        pty::grantpt(&master_fd).map_err(|e| format!("grantpt failed: {}", e))?;
        pty::unlockpt(&master_fd).map_err(|e| format!("unlockpt failed: {}", e))?;

        // Get the slave PTY name (use ptsname, which is available on macOS)
        let slave_name =
            unsafe { pty::ptsname(&master_fd) }.map_err(|e| format!("ptsname failed: {}", e))?;

        // Fork
        let pid = unsafe { nix::unistd::fork() }
            .map_err(|e| format!("fork failed: {}", e))?;

        match pid {
            nix::unistd::ForkResult::Child => {
                // ── Child process ──
                // Create a new session and become session leader
                setsid()
                    .map_err(|e| {
                        eprintln!("setsid failed: {}", e);
                    })
                    .ok();

                // Open the slave PTY
                let slave_fd = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&slave_name)
                    .expect("Failed to open slave PTY");
                let slave_raw = slave_fd.as_raw_fd();

                // Duplicate slave fd to stdin, stdout, stderr
                dup2(slave_raw, 0).expect("dup2 stdin failed");
                dup2(slave_raw, 1).expect("dup2 stdout failed");
                dup2(slave_raw, 2).expect("dup2 stderr failed");

                // Close the master fd in the child
                let _ = close(master_raw);
                drop(slave_fd);

                // Set terminal size
                let ws = nix::libc::winsize {
                    ws_row: rows,
                    ws_col: cols,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                unsafe {
                    nix::libc::ioctl(0, nix::libc::TIOCSWINSZ, &ws);
                }

                // Set TERM environment variable
                std::env::set_var("TERM", "xterm-256color");

                // Apply custom env vars
                if let Some(env_vars) = env {
                    for (key, value) in env_vars {
                        std::env::set_var(&key, &value);
                    }
                }

                // Change directory if specified
                if let Some(dir) = cwd {
                    std::env::set_current_dir(&dir).ok();
                }

                // Execute the shell
                let err = std::process::Command::new(&shell)
                    .spawn()
                    .expect("Failed to spawn shell")
                    .wait();

                std::process::exit(err.map(|s| s.code().unwrap_or(1)).unwrap_or(1));
            }
            nix::unistd::ForkResult::Parent { child } => {
                // ── Parent process ──
                let pid = child.as_raw() as u32;

                // Set the master fd to non-blocking for reads
                let flags =
                    fcntl(master_raw, FcntlArg::F_GETFL).map_err(|e| format!("fcntl(F_GETFL) failed: {}", e))?;
                fcntl(
                    master_raw,
                    FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
                )
                .map_err(|e| format!("fcntl(F_SETFL) failed: {}", e))?;

                // Consume the PtyMaster to get the raw fd, then wrap in OwnedFd.
                // This prevents double-close: the PtyMaster is consumed and won't
                // drop the fd when it goes out of scope.
                let owned_fd = unsafe { OwnedFd::from_raw_fd(master_fd.into_raw_fd()) };

                let session = TerminalSession {
                    id: id.clone(),
                    shell: shell.clone(),
                    pid,
                    master_fd: owned_fd,
                    cols,
                    rows,
                    created_at,
                    status: "running".to_string(),
                    output: Vec::new(),
                    max_output_lines: self.max_output_lines,
                };

                let mut sessions = self.sessions.lock().await;
                sessions.insert(id.clone(), session);

                Ok(SpawnResult {
                    session_id: id,
                    pid,
                    shell,
                    cols,
                    rows,
                    created_at,
                })
            }
        }
    }

    /// Write data to a terminal session.
    pub async fn write(&self, session_id: &str, input: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;
        session.write(input).await
    }

    /// Read pending output from a terminal session.
    pub async fn read(
        &self,
        session_id: &str,
        max_bytes: Option<usize>,
    ) -> Result<ReadResult, String> {
        let max_bytes = max_bytes.unwrap_or(65536);
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;
        session.read(max_bytes).await
    }

    /// Resize a terminal session.
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;
        session.resize(cols, rows)
    }

    /// Kill a terminal session.
    pub async fn kill(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        if session.status != "running" {
            return Err(format!(
                "Session {} is not running (status: {})",
                session_id, session.status
            ));
        }

        // Send SIGTERM to the child process
        let pid = Pid::from_raw(session.pid as i32);
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM);

        session.status = "terminated".to_string();
        Ok(())
    }

    /// List all active sessions.
    pub async fn list(&self) -> ListResult {
        let sessions = self.sessions.lock().await;
        let infos: Vec<SessionInfo> = sessions
            .values()
            .map(|s| SessionInfo {
                session_id: s.id.clone(),
                shell: s.shell.clone(),
                pid: s.pid,
                cols: s.cols,
                rows: s.rows,
                created_at: s.created_at,
                status: s.status.clone(),
                output_lines: s.output.len(),
            })
            .collect();
        ListResult { sessions: infos }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_list() {
        let manager = TerminalManager::new();
        let result = manager.spawn(None, None, None, None, None).await.unwrap();
        assert_eq!(result.shell, "/bin/zsh");
        assert!(result.pid > 0);

        let list = manager.list().await;
        assert!(!list.sessions.is_empty());
        assert!(list
            .sessions
            .iter()
            .any(|s| s.session_id == result.session_id));
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let manager = TerminalManager::new();
        let result = manager.spawn(None, None, None, None, None).await.unwrap();

        // Write a simple echo command
        manager
            .write(&result.session_id, "echo 'hello from pty'\n")
            .await
            .unwrap();

        // Give it time to execute
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let read_result = manager.read(&result.session_id, Some(4096)).await.unwrap();
        assert!(
            read_result.data.contains("hello from pty"),
            "Expected 'hello from pty' in output, got: {:?}",
            read_result.data
        );
    }

    #[tokio::test]
    async fn test_resize() {
        let manager = TerminalManager::new();
        let result = manager
            .spawn(None, None, None, Some(80), Some(24))
            .await
            .unwrap();

        manager
            .resize(&result.session_id, 132, 43)
            .await
            .unwrap();

        let list = manager.list().await;
        let session = list
            .sessions
            .iter()
            .find(|s| s.session_id == result.session_id)
            .unwrap();
        assert_eq!(session.cols, 132);
        assert_eq!(session.rows, 43);
    }

    #[tokio::test]
    async fn test_kill() {
        let manager = TerminalManager::new();
        let result = manager.spawn(None, None, None, None, None).await.unwrap();

        manager.kill(&result.session_id).await.unwrap();

        let list = manager.list().await;
        let session = list
            .sessions
            .iter()
            .find(|s| s.session_id == result.session_id)
            .unwrap();
        assert_eq!(session.status, "terminated");
    }

    #[tokio::test]
    async fn test_custom_shell() {
        let manager = TerminalManager::new();
        let result = manager
            .spawn(Some("/bin/bash".to_string()), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(result.shell, "/bin/bash");
    }

    #[tokio::test]
    async fn test_custom_cwd() {
        let manager = TerminalManager::new();
        let tmp = std::env::temp_dir();
        let result = manager
            .spawn(None, Some(tmp.to_string_lossy().to_string()), None, None, None)
            .await
            .unwrap();

        // Wait for shell to be ready
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Drain any initial output (shell prompt)
        let _ = manager.read(&result.session_id, Some(65536)).await;

        // Check the working directory
        manager.write(&result.session_id, "pwd\n").await.unwrap();
        // Wait longer for the command to execute
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

        // Read multiple times to collect all output
        let mut all_output = String::new();
        for _ in 0..5 {
            let read_result = manager.read(&result.session_id, Some(4096)).await.unwrap();
            all_output.push_str(&read_result.data);
            if all_output.contains("/") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        let tmp_str = tmp.to_string_lossy().to_string();
        // Handle /var -> /private/var symlink on macOS
        let canonical = std::fs::canonicalize(&tmp)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| tmp_str.clone());
        assert!(
            all_output.contains(&canonical) || all_output.contains(&tmp_str),
            "Expected pwd to contain {:?} or {:?}, got: {:?}",
            tmp_str,
            canonical,
            all_output
        );
    }
}
