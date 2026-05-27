//! Security sandbox for Rupoo system operations.
//!
//! Provides command validation, path restrictions, and timeout configuration
//! to prevent dangerous operations (e.g., `sudo rm -rf /`, SSRF attacks).
//!
//! # Safety notes
//! - All external command executions must pass through `validate_command` first.
//! - File access uses `path_jail` for OS-level path traversal prevention.
//! - Default timeouts prevent runaway processes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;


use tracing::warn;

use crate::error::{AgentError, AgentResult};

/// Security context that governs all system-level operations.
///
/// Configuration is loaded from `rupoo-config.toml` if present; otherwise
/// built-in safe defaults are used.
#[derive(Debug, Clone)]
pub struct SafetyContext {
    /// Commands that are permanently forbidden (e.g., `sudo`, `rm`, `mkfs`).
    forbidden_commands: HashSet<String>,
    /// Directories the agent is allowed to read/write.
    allowed_paths: Vec<PathBuf>,
    /// Default timeout for external command execution.
    pub default_timeout: Duration,
    /// Optional explicit path to browser executable.
    pub browser_path: Option<PathBuf>,
}

impl Default for SafetyContext {
    fn default() -> Self {
        Self {
            forbidden_commands: [
                "sudo", "su", "passwd", "chown", "chmod", "chattr",
                "rm", "mkfs", "fdisk", "dd", "format",
                "shutdown", "reboot", "halt", "poweroff",
                "kill", "killall", "pkill",
                "iptables", "ufw",
                "mount", "umount",
                // Shell interpreters — can execute arbitrary commands bypassing the blacklist
                "sh", "bash", "zsh", "dash", "ksh", "csh", "tcsh", "fish",
                // Script interpreters — same risk as shells
                "python", "python3", "perl", "ruby", "node", "lua",
                // Direct binary execution paths
                "/bin/sh", "/bin/bash", "/bin/zsh", "/usr/bin/env",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            allowed_paths: vec![
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            ],
            default_timeout: Duration::from_secs(30),
            browser_path: None,
        }
    }
}

impl SafetyContext {
    /// Load configuration from a TOML file. Falls back to defaults on error.
    pub fn from_config(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "failed to read safety config, using defaults");
                return Self::default();
            }
        };
        let parsed: Option<SafetyConfig> = toml::from_str(&content).ok();
        match parsed {
            Some(cfg) => Self {
                forbidden_commands: cfg.forbidden_commands.into_iter().collect(),
                allowed_paths: cfg.allowed_paths.into_iter().map(PathBuf::from).collect(),
                default_timeout: Duration::from_secs(cfg.default_timeout_secs),
                browser_path: cfg.browser_path.map(PathBuf::from),
            },
            None => Self::default(),
        }
    }

    /// Check if a command is allowed to run.
    ///
    /// Returns `Ok(())` if the command is safe, or `Err` with a description
    /// if the command is blacklisted.
    pub fn validate_command(&self, command: &str) -> AgentResult<()> {
        let base = command.split_whitespace().next().unwrap_or(command);
        let base_lower = base.to_lowercase();
        if self.forbidden_commands.contains(&base_lower) {
            warn!(command = %base, "blocked forbidden command");
            return Err(AgentError::Other(format!(
                "Command '{}' is forbidden by security policy",
                base
            )));
        }
        Ok(())
    }

    /// Resolve a path under the file sandbox.
    ///
    /// Uses `path_jail` for OS-level path traversal prevention.
    /// Blocks `../../etc/passwd`, symlink escapes, absolute path injection.
    ///
    /// # Security
    /// path_jail provides zero-dependency protection against:
    /// - Path traversal (`../../etc/passwd`)
    /// - Symlink escapes (symlinks pointing outside the jail)
    /// - Absolute path injection (`/etc/passwd`)
    /// - Null byte injection (`file\x00.txt`)
    pub fn apply_file_jail(&self, path: &Path) -> AgentResult<PathBuf> {
        if self.allowed_paths.is_empty() {
            return Ok(path.to_path_buf());
        }
        // Use the first allowed path as the jail root
        let root = &self.allowed_paths[0];
        let root_canonical = std::fs::canonicalize(root)
            .unwrap_or_else(|e| {
                warn!(error = %e, path = %root.display(), "failed to canonicalize jail root");
                root.to_path_buf()
            });

        // path_jail::join validates the path against traversal attacks
        path_jail::join(&root_canonical, path).map_err(|e| {
            AgentError::Other(format!(
                "Access denied to '{}': {e}",
                path.display()
            ))
        })
    }

    /// Check if a URL points to localhost (SSRF protection).
    /// Performs string-based heuristic check (fast path).
    /// For thorough protection, use `is_private_host` which resolves DNS.
    pub fn is_localhost_url(url: &str) -> bool {
        let lower = url.to_lowercase();
        // Direct localhost addresses
        if lower.starts_with("http://localhost")
            || lower.starts_with("https://localhost")
            || lower.starts_with("http://127.0.0.1")
            || lower.starts_with("https://127.0.0.1")
            || lower.starts_with("http://[::1]")
            || lower.starts_with("https://[::1]")
            || lower.starts_with("http://0.0.0.0")
            || lower.starts_with("https://0.0.0.0")
            || lower.starts_with("http://[0:0:0:0:0:0:0:1]")
            || lower.starts_with("https://[0:0:0:0:0:0:0:1]")
        {
            return true;
        }
        // Cloud metadata IP (169.254.x.x range)
        if lower.starts_with("http://169.254.") || lower.starts_with("https://169.254.") {
            return true;
        }
        // DNS rebinding domains that resolve arbitrary IPs
        if lower.contains("nip.io") || lower.contains("xip.io") || lower.contains("sslip.io") {
            return true;
        }
        false
    }

    /// Resolve a hostname via DNS and check if any resolved IP is private/local.
    /// Returns `true` if the host resolves to a private IP (SSRF risk).
    ///
    /// This prevents DNS rebinding attacks where `evil.com` resolves to 127.0.0.1
    /// after passing the string-based `is_localhost_url` check.
    pub async fn is_private_host(host: &str) -> bool {
        // Fast path: known local strings don't need DNS lookup
        let lower = host.to_lowercase();
        if lower == "localhost" || lower == "127.0.0.1" || lower == "::1" || lower == "0.0.0.0" {
            return true;
        }

        // DNS resolution
        match tokio::net::lookup_host(format!("{host}:0")).await {
            Ok(addrs) => {
                for addr in addrs {
                    let ip = addr.ip();
                    let is_private = match ip {
                        std::net::IpAddr::V4(v4) => {
                            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
                        }
                        std::net::IpAddr::V6(v6) => {
                            v6.is_loopback() || v6.is_unspecified()
                        }
                    };
                    if is_private {
                        warn!(
                            host = %host,
                            ip = %ip,
                            "host resolves to private IP — SSRF blocked"
                        );
                        return true;
                    }
                }
                false
            }
            Err(e) => {
                // If DNS resolution fails, be conservative and block
                warn!(host = %host, error = %e, "DNS resolution failed — blocking for safety");
                true
            }
        }
    }

    /// Return the primary jail root path, if configured.
    pub fn jail_root(&self) -> Option<&std::path::Path> {
        self.allowed_paths.first().map(|p| p.as_path())
    }

    /// Check if a tool call requires user approval before execution.
    ///
    /// Returns `true` for high-risk operations (file deletion, network calls
    /// to sensitive targets, system configuration changes).
    pub fn needs_approval(&self, tool_name: &str) -> bool {
        // High-risk tool names that require explicit user approval.
        // Extend this list as needed — the list is intentionally small to
        // avoid alert fatigue; most tools are auto-approved by default.
        matches!(
            tool_name.to_lowercase().as_str(),
            "delete_file"
                | "rm"
                | "remove"
                | "exec"
                | "run_command"
                | "bash"
                | "shell"
                | "sudo"
                | "reboot"
                | "shutdown"
                | "http_delete"
                | "http_post"
        )
    }
}

/// TOML-deserializable safety configuration.
#[derive(serde::Deserialize)]
struct SafetyConfig {
    #[serde(default)]
    forbidden_commands: Vec<String>,
    #[serde(default)]
    allowed_paths: Vec<String>,
    #[serde(default = "default_timeout")]
    default_timeout_secs: u64,
    browser_path: Option<String>,
}

fn default_timeout() -> u64 {
    30
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_forbidden_command() {
        let ctx = SafetyContext::default();
        assert!(ctx.validate_command("sudo rm -rf /").is_err());
        assert!(ctx.validate_command("rm -rf /").is_err());
        assert!(ctx.validate_command("echo hello").is_ok());
    }

    #[test]
    fn test_localhost_detection() {
        assert!(SafetyContext::is_localhost_url("http://localhost:8080"));
        assert!(SafetyContext::is_localhost_url("http://127.0.0.1/api"));
        assert!(!SafetyContext::is_localhost_url("http://example.com"));
    }

    #[test]
    fn test_default_timeout() {
        let ctx = SafetyContext::default();
        assert_eq!(ctx.default_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_path_traversal_blocked() {
        // Create a temp dir to serve as the jail root
        let tmp = tempfile::tempdir().unwrap();
        let jail_root = tmp.path().join("allowed_dir");
        fs::create_dir(&jail_root).unwrap();

        let ctx = SafetyContext {
            allowed_paths: vec![jail_root.clone()],
            ..SafetyContext::default()
        };

        // Path traversal attempts should be blocked
        let traversal = PathBuf::from("../../../etc/passwd");
        assert!(ctx.apply_file_jail(&traversal).is_err());

        // Absolute paths should be blocked
        let absolute = PathBuf::from("/etc/passwd");
        assert!(ctx.apply_file_jail(&absolute).is_err());

        // Allowed path should work
        let allowed = PathBuf::from("test.txt");
        assert!(ctx.apply_file_jail(&allowed).is_ok());
    }

    #[test]
    fn test_null_byte_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let jail_root = tmp.path().join("allowed");
        fs::create_dir(&jail_root).unwrap();

        let ctx = SafetyContext {
            allowed_paths: vec![jail_root.clone()],
            ..SafetyContext::default()
        };

        // Null byte injection should be blocked
        let null_byte = PathBuf::from("file\x00.txt");
        assert!(ctx.apply_file_jail(&null_byte).is_err());
    }
}
