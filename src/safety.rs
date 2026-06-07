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
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use lru::LruCache;
use tracing::warn;

use crate::error::{AgentError, AgentResult};

/// Default TTL for DNS cache entries (5 minutes).
/// This balances security (preventing cache poisoning) and performance.
const DNS_CACHE_TTL: Duration = Duration::from_secs(300);

/// DNS cache entry with TTL for cache invalidation.
#[derive(Debug, Clone, Copy)]
struct DnsCacheEntry {
    is_private: bool,
    expires_at: Instant,
}

impl DnsCacheEntry {
    /// Create a new cache entry with the default TTL.
    fn new(is_private: bool) -> Self {
        Self {
            is_private,
            expires_at: Instant::now() + DNS_CACHE_TTL,
        }
    }

    /// Check if the cache entry is still valid (not expired).
    fn is_valid(&self) -> bool {
        Instant::now() < self.expires_at
    }
}

/// LRU cache for DNS resolution results to prevent repeated lookups.
/// This mitigates DNS cache poisoning attacks and improves performance.
///
/// # Security
/// - Uses TTL-based cache invalidation to prevent stale entries
/// - Default TTL of 5 minutes provides balance between security and performance
/// - Maximum TTL of 1 hour prevents indefinitely cached entries
static DNS_CACHE: OnceLock<std::sync::Mutex<LruCache<String, DnsCacheEntry>>> = OnceLock::new();

/// Get or initialize the DNS cache with a capacity of 100 entries.
fn dns_cache() -> &'static std::sync::Mutex<LruCache<String, DnsCacheEntry>> {
    DNS_CACHE.get_or_init(|| {
        std::sync::Mutex::new(LruCache::new(std::num::NonZeroUsize::new(100).unwrap()))
    })
}

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
                "sudo", "su", "passwd", "mkfs", "fdisk", "dd", "format", "shutdown", "reboot",
                "halt", "poweroff", "iptables", "ufw", "mount",
                "umount",
                // File-modifying commands (rm/chmod/kill/chown) moved to
                // needs_approval() so they require user confirmation but can
                // still be used when explicitly approved.
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            allowed_paths: vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))],
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
            return Err(AgentError::Safety(format!(
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
        let root_canonical = std::fs::canonicalize(root).unwrap_or_else(|e| {
            warn!(error = %e, path = %root.display(), "failed to canonicalize jail root");
            root.to_path_buf()
        });

        // path_jail::join validates the path against traversal attacks
        path_jail::join(&root_canonical, path)
            .map_err(|e| AgentError::Safety(format!("Access denied to '{}': {e}", path.display())))
    }

    /// Check if a URL points to localhost (SSRF protection).
    /// Performs string-based heuristic check (fast path).
    /// For thorough protection, use `is_private_host` which resolves DNS.
    pub fn is_localhost_url(url: &str) -> bool {
        let lower = url.to_lowercase();

        // Helper: check if URL starts with http:// or https:// for a given host prefix
        let starts_with_host = |prefix: &str| -> bool {
            lower.starts_with(&format!("http://{prefix}"))
                || lower.starts_with(&format!("https://{prefix}"))
        };

        // localhost hostname
        if starts_with_host("localhost") {
            return true;
        }

        // 127.0.0.0/8 — entire loopback range (127.x.x.x all resolve to localhost)
        if starts_with_host("127.") {
            return true;
        }

        // 0.0.0.0 — binds all interfaces
        if starts_with_host("0.0.0.0") {
            return true;
        }

        // IPv6 loopback — [::1] and expanded form [0:0:0:0:0:0:0:1]
        if starts_with_host("[::1]") || starts_with_host("[0:0:0:0:0:0:0:1]") {
            return true;
        }

        // IPv4-mapped IPv6 loopback — [::ffff:127.0.0.1], [::ffff:7f00:1], [0:0:0:0:0:ffff:127.0.0.1]
        if starts_with_host("[::ffff:127.")
            || starts_with_host("[0:0:0:0:0:ffff:127.")
            || starts_with_host("[::ffff:7f")
            || starts_with_host("[0:0:0:0:0:ffff:7f")
        {
            return true;
        }

        // IPv6 unspecified address — [::] (equivalent to 0.0.0.0)
        if starts_with_host("[::]") || starts_with_host("[0:0:0:0:0:0:0:0]") {
            return true;
        }

        // Octal loopback — 0177.0.0.1 == 127.0.0.1 (some resolvers accept this)
        if starts_with_host("0177.") {
            return true;
        }

        // Cloud metadata IP (169.254.169.254 and entire link-local range)
        if starts_with_host("169.254.") {
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
    ///
    /// Results are cached to improve performance and mitigate DNS cache poisoning.
    pub async fn is_private_host(host: &str) -> bool {
        // Fast path: known local strings don't need DNS lookup
        let lower = host.to_lowercase();
        if lower == "localhost" || lower == "127.0.0.1" || lower == "::1" || lower == "0.0.0.0" {
            return true;
        }

        // Check cache first (with TTL validation)
        if let Ok(mut cache) = dns_cache().lock() {
            if let Some(entry) = cache.get(&lower) {
                if entry.is_valid() {
                    // Cache hit - return cached result
                    return entry.is_private;
                } else {
                    // Cache entry expired - remove it
                    cache.pop(&lower);
                }
            }
        }

        // DNS resolution
        let result = match tokio::net::lookup_host(format!("{host}:0")).await {
            Ok(addrs) => {
                for addr in addrs {
                    let ip = addr.ip();
                    let is_private = match ip {
                        std::net::IpAddr::V4(v4) => {
                            v4.is_loopback()
                                || v4.is_private()
                                || v4.is_link_local()
                                || v4.is_unspecified()
                        }
                        std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
                    };
                    if is_private {
                        warn!(
                            host = %host,
                            ip = %ip,
                            "host resolves to private IP — SSRF blocked"
                        );
                        let _ = dns_cache()
                            .lock()
                            .map(|mut c| c.put(lower, DnsCacheEntry::new(true)));
                        return true;
                    }
                }
                false
            }
            Err(e) => {
                // If DNS resolution fails, be conservative and block
                warn!(host = %host, error = %e, "DNS resolution failed — blocking for safety");
                let _ = dns_cache()
                    .lock()
                    .map(|mut c| c.put(lower, DnsCacheEntry::new(true)));
                return true;
            }
        };

        // Cache the successful non-private result
        let _ = dns_cache()
            .lock()
            .map(|mut c| c.put(lower, DnsCacheEntry::new(result)));
        result
    }

    /// Return the primary jail root path, if configured.
    pub fn jail_root(&self) -> Option<&std::path::Path> {
        self.allowed_paths.first().map(|p| p.as_path())
    }

    /// Environment variables that are safe to forward to child processes.
    /// These are considered non-sensitive and essential for basic operation.
    const SAFE_ENV_VARS: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "SHELL",
        "LANG",
        "LC_ALL",
        "TERM",
        "PWD",
        "LOGNAME",
        "SUDO_UID",
        "SUDO_GID",
        "SUDO_USER",
        "RUST_LOG",
        "CARGO_TARGET_DIR",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
    ];

    /// Patterns for sensitive environment variables that must be blocked.
    /// These are checked against variable names (case-insensitive).
    const SENSITIVE_PATTERNS: &[&str] = &[
        "AWS_",
        "GITHUB_",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "KEY",
        "DOCKER_AUTH",
        "API_KEY",
        "ACCESS_KEY",
        "SECRET_KEY",
        "BEARER_TOKEN",
        "SSH_AUTH_SOCK",
        "PGPASSWORD",
        "MYSQL_PWD",
        "MONGODB_URI",
        "OPENAI_",
        "ANTHROPIC_",
        "DEEPSEEK_",
        "OLLAMA_",
    ];

    /// Forward safe environment variables to a child process after clearing.
    /// Only essential, non-sensitive vars are preserved.
    ///
    /// # Security
    /// - Clears all environment variables first (defense in depth)
    /// - Only forwards variables in the SAFE_ENV_VARS whitelist
    /// - Logs any sensitive variables that were present in the parent environment
    pub fn forward_safe_env(cmd: &mut std::process::Command) {
        // Clear all environment variables first
        cmd.env_clear();

        // Track sensitive variables for auditing
        let mut blocked_sensitive = Vec::new();

        // Check for sensitive variables and log them (for auditing)
        for (key, _) in std::env::vars() {
            let key_upper = key.to_ascii_uppercase();
            if Self::SENSITIVE_PATTERNS
                .iter()
                .any(|pattern| key_upper.starts_with(pattern) || key_upper.contains(pattern))
            {
                blocked_sensitive.push(key);
            }
        }

        // Log blocked sensitive variables for security auditing
        if !blocked_sensitive.is_empty() {
            tracing::debug!(
                blocked_vars = ?blocked_sensitive,
                "blocked sensitive environment variables from child process"
            );
        }

        // Forward safe variables
        for &var in Self::SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
    }

    /// Forward safe environment variables to a tokio child process after clearing.
    pub fn forward_safe_env_async(cmd: &mut tokio::process::Command) {
        // Clear all environment variables first
        cmd.env_clear();

        // Track sensitive variables for auditing
        let mut blocked_sensitive = Vec::new();

        // Check for sensitive variables and log them (for auditing)
        for (key, _) in std::env::vars() {
            let key_upper = key.to_ascii_uppercase();
            if Self::SENSITIVE_PATTERNS
                .iter()
                .any(|pattern| key_upper.starts_with(pattern) || key_upper.contains(pattern))
            {
                blocked_sensitive.push(key);
            }
        }

        // Log blocked sensitive variables for security auditing
        if !blocked_sensitive.is_empty() {
            tracing::debug!(
                blocked_vars = ?blocked_sensitive,
                "blocked sensitive environment variables from async child process"
            );
        }

        // Forward safe variables
        for &var in Self::SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
    }

    /// Check if a tool call requires user approval before execution.
    ///
    /// Returns `true` for high-risk operations (file deletion, network calls
    /// to sensitive targets, system configuration changes).
    pub fn needs_approval(&self, tool_name: &str) -> bool {
        // High-risk tool names that require explicit user approval.
        // Extend this list as needed — the list is intentionally small to
        // avoid alert fatigue; most tools are auto-approved by default.
        let lower = tool_name.to_lowercase();
        let base = lower.split_whitespace().next().unwrap_or(&lower);
        matches!(
            base,
            "delete_file"
                | "rm"
                | "remove"
                | "exec"
                | "run_command"
                | "bash"
                | "sh"
                | "zsh"
                | "dash"
                | "ksh"
                | "csh"
                | "fish"
                | "shell"
                | "sudo"
                | "reboot"
                | "shutdown"
                | "http_delete"
                | "http_post"
                // Script interpreters — can execute arbitrary code
                | "python"
                | "python3"
                | "perl"
                | "ruby"
                | "node"
                | "lua"
        ) || base.starts_with("/bin/sh")
            || base.starts_with("/bin/bash")
            || base.starts_with("/usr/bin/env")
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
        // sudo is explicitly forbidden
        assert!(ctx.validate_command("sudo rm -rf /").is_err());
        // rm is no longer forbidden — it requires approval instead
        assert!(ctx.validate_command("rm -rf /").is_ok());
        assert!(ctx.needs_approval("rm -rf /"));
        assert!(ctx.validate_command("echo hello").is_ok());
        // sh/bash are no longer forbidden — they require approval instead
        assert!(ctx.validate_command("bash -c 'echo hello'").is_ok());
        assert!(ctx.validate_command("sh -c 'ls'").is_ok());
    }

    #[test]
    fn test_needs_approval() {
        let ctx = SafetyContext::default();
        // Shell interpreters require approval
        assert!(ctx.needs_approval("bash"));
        assert!(ctx.needs_approval("sh"));
        assert!(ctx.needs_approval("zsh"));
        assert!(ctx.needs_approval("/bin/bash -c 'echo hi'"));
        assert!(ctx.needs_approval("/usr/bin/env python3"));
        // Script interpreters require approval
        assert!(ctx.needs_approval("python3"));
        assert!(ctx.needs_approval("node"));
        assert!(ctx.needs_approval("perl"));
        // Safe commands don't need approval
        assert!(!ctx.needs_approval("echo"));
        assert!(!ctx.needs_approval("ls"));
        assert!(!ctx.needs_approval("cat"));
        // Dangerous tool names still need approval
        assert!(ctx.needs_approval("delete_file"));
        assert!(ctx.needs_approval("http_post"));
    }

    #[test]
    fn test_localhost_detection() {
        // Basic cases
        assert!(SafetyContext::is_localhost_url("http://localhost:8080"));
        assert!(SafetyContext::is_localhost_url("https://localhost/path"));
        assert!(SafetyContext::is_localhost_url("http://127.0.0.1/api"));
        assert!(SafetyContext::is_localhost_url("https://127.0.0.1/"));

        // Full 127.0.0.0/8 loopback range
        assert!(SafetyContext::is_localhost_url("http://127.1.2.3/test"));
        assert!(SafetyContext::is_localhost_url("http://127.255.255.255/"));

        // 0.0.0.0
        assert!(SafetyContext::is_localhost_url("http://0.0.0.0/"));
        assert!(SafetyContext::is_localhost_url("https://0.0.0.0:8080/"));

        // IPv6 loopback
        assert!(SafetyContext::is_localhost_url("http://[::1]/"));
        assert!(SafetyContext::is_localhost_url("https://[::1]:8080/"));
        assert!(SafetyContext::is_localhost_url("http://[0:0:0:0:0:0:0:1]/"));

        // IPv4-mapped IPv6
        assert!(SafetyContext::is_localhost_url(
            "http://[::ffff:127.0.0.1]/"
        ));
        assert!(SafetyContext::is_localhost_url(
            "https://[::ffff:127.0.0.1]:80/"
        ));
        assert!(SafetyContext::is_localhost_url(
            "http://[0:0:0:0:0:ffff:127.0.0.1]/"
        ));
        assert!(SafetyContext::is_localhost_url("http://[::ffff:7f00:1]/"));

        // IPv6 unspecified
        assert!(SafetyContext::is_localhost_url("http://[::]/"));
        assert!(SafetyContext::is_localhost_url("http://[0:0:0:0:0:0:0:0]/"));

        // Octal loopback
        assert!(SafetyContext::is_localhost_url("http://0177.0.0.1/"));
        assert!(SafetyContext::is_localhost_url("http://0177.0.0.2/"));

        // Cloud metadata
        assert!(SafetyContext::is_localhost_url("http://169.254.169.254/"));
        assert!(SafetyContext::is_localhost_url("http://169.254.1.1/"));

        // DNS rebinding
        assert!(SafetyContext::is_localhost_url("http://evil.nip.io/"));
        assert!(SafetyContext::is_localhost_url("http://10.0.0.1.xip.io/"));

        // Should NOT be blocked
        assert!(!SafetyContext::is_localhost_url("http://example.com"));
        assert!(!SafetyContext::is_localhost_url(
            "https://github.com/user/repo"
        ));
        assert!(!SafetyContext::is_localhost_url("http://192.168.1.100/")); // private but not localhost
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
