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
use std::sync::{LazyLock, OnceLock};
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
        std::sync::Mutex::new(LruCache::new(
            // SAFETY: 100 is a compile-time constant greater than zero
            std::num::NonZeroUsize::new(100).unwrap_or(std::num::NonZeroUsize::MIN),
        ))
    })
}

/// Cached default sets — built once, cloned on each `SafetyContext::default()`.
/// This avoids re-allocating HashSets on every tool call (rig_tools creates a
/// SafetyContext per invocation).
static DEFAULT_FORBIDDEN: LazyLock<HashSet<String>> = LazyLock::new(|| {
    [
        "sudo", "su", "passwd", "mkfs", "fdisk", "dd", "format", "shutdown", "reboot", "halt",
        "poweroff", "iptables", "ufw", "mount", "umount", "chown", "chmod", "chattr", "kill",
        "killall", "pkill", "rm",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
});

static DEFAULT_APPROVAL: LazyLock<HashSet<String>> = LazyLock::new(|| {
    [
        "delete_file",
        "rm",
        "remove",
        "exec",
        "run_command",
        "bash",
        "sh",
        "zsh",
        "dash",
        "ksh",
        "csh",
        "fish",
        "shell",
        "sudo",
        "reboot",
        "shutdown",
        "http_delete",
        "http_post",
        "python",
        "python3",
        "perl",
        "ruby",
        "node",
        "lua",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
});

static DEFAULT_AUTO_APPROVE: LazyLock<HashSet<String>> = LazyLock::new(|| {
    [
        "echo",
        "file_read",
        "list_directory",
        "file_write",
        "file_edit",
        "code_search",
        "run_tests",
        "check_output",
        "diff_check",
        "web_search",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
});

/// Security context that governs all system-level operations.
///
/// Built-in safe defaults are used for all security settings.
#[derive(Debug, Clone)]
pub struct SafetyContext {
    /// Commands that are permanently forbidden (e.g., `sudo`, `rm`, `mkfs`).
    forbidden_commands: HashSet<String>,
    /// Tools that require user approval before execution.
    approval_required_tools: HashSet<String>,
    /// Tools that can run without user approval.
    /// Reserved for integration with the approval policy system.
    #[allow(dead_code)]
    auto_approve_tools: HashSet<String>,
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
            forbidden_commands: DEFAULT_FORBIDDEN.clone(),
            approval_required_tools: DEFAULT_APPROVAL.clone(),
            auto_approve_tools: DEFAULT_AUTO_APPROVE.clone(),
            allowed_paths: vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))],
            default_timeout: Duration::from_secs(30),
            browser_path: None,
        }
    }
}

impl SafetyContext {
    /// Create a SafetyContext from configuration, merging with built-in defaults.
    ///
    /// Config values extend (not replace) the hardcoded defaults, so removing a
    /// default-forbidden command requires an explicit allowlist (future feature).
    pub fn from_config(config: &crate::config::RupooConfig) -> Self {
        let mut ctx = SafetyContext::default();

        // Extend forbidden commands from config (additive only)
        for cmd in &config.safety.forbidden_commands {
            if !cmd.is_empty() {
                ctx.forbidden_commands.insert(cmd.to_lowercase());
            }
        }

        // Use config's jail_root if explicitly set (overrides default ".")
        if config.safety.jail_root != "." && !config.safety.jail_root.is_empty() {
            if let Ok(root) = std::fs::canonicalize(&config.safety.jail_root) {
                ctx.allowed_paths.insert(0, root);
            }
        }

        // Extend auto-approve tools from config
        for tool in &config.safety.auto_approve_tools {
            ctx.auto_approve_tools.insert(tool.to_string());
        }

        ctx
    }

    /// Check if a command is allowed to run.
    ///
    /// Returns `Ok(())` if the command is safe, or `Err` with a description
    /// if the command is blacklisted.
    pub fn validate_command(&self, command: &str) -> AgentResult<()> {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        if tokens.is_empty() {
            return Ok(());
        }
        // 透过 `env` / `command` 包装器取真实程序名（如 `env rm -rf /` → rm）。
        let base = match tokens[0].to_ascii_lowercase().as_str() {
            "env" | "command" if tokens.len() > 1 => tokens[1],
            _ => tokens[0],
        };

        // 候选名集合：字面 base、其文件名（若为路径）、经 PATH 解析后的真实二进制名。
        // 这样 `/usr/bin/rm`、`env rm`、相对路径 `./rm` 都无法绕过黑名单。
        let mut candidates: Vec<String> = Vec::new();
        let base_lower = base.to_lowercase();
        candidates.push(base_lower.clone());
        if base_lower.contains('/') {
            if let Some(name) = Path::new(&base_lower).file_name() {
                candidates.push(name.to_string_lossy().to_lowercase().to_string());
            }
        }
        if let Some(resolved) = Self::resolve_in_path(base) {
            if let Some(name) = Path::new(&resolved).file_name() {
                candidates.push(name.to_string_lossy().to_lowercase().to_string());
            }
        }

        for cand in &candidates {
            if self.forbidden_commands.contains(cand) {
                warn!(command = %base, "blocked forbidden command");
                return Err(AgentError::Safety(format!(
                    "Command '{}' is forbidden by security policy",
                    base
                )));
            }
        }
        Ok(())
    }

    /// 解析命令的真实路径（类似 `which`），用于黑名单比对，防止通过绝对/相对路径绕过。
    /// 返回解析后的完整路径（若存在），否则 `None`。
    fn resolve_in_path(cmd: &str) -> Option<String> {
        if cmd.contains('/') {
            let p = Path::new(cmd);
            if p.is_absolute() {
                return Some(cmd.to_string());
            }
            // 相对路径：基于当前目录解析
            if let Ok(cwd) = std::env::current_dir() {
                let joined = cwd.join(cmd);
                if joined.is_file() {
                    return Some(joined.to_string_lossy().to_string());
                }
            }
            return None;
        }
        let path_env = std::env::var("PATH").unwrap_or_default();
        for dir in path_env.split(':') {
            let candidate = Path::new(dir).join(cmd);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
        None
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
                        std::net::IpAddr::V6(v6) => {
                            v6.is_loopback() || v6.is_unspecified() || v6.is_unicast_link_local()
                        }
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

    /// Export forbidden commands as a Vec for use in compliance checking.
    pub fn forbidden_commands(&self) -> Vec<String> {
        self.forbidden_commands.iter().cloned().collect()
    }

    /// Check if a tool call requires user approval before execution.
    ///
    /// Returns `true` for high-risk operations (file deletion, network calls
    /// to sensitive targets, system configuration changes).
    pub fn needs_approval(&self, tool_name: &str) -> bool {
        let lower = tool_name.to_lowercase();
        let base = lower.split_whitespace().next().unwrap_or(&lower);
        // Check configured approval list
        if self.approval_required_tools.contains(base) {
            return true;
        }
        // Check path patterns (e.g., /bin/bash, /usr/bin/env)
        if base.starts_with("/bin/sh")
            || base.starts_with("/bin/bash")
            || base.starts_with("/usr/bin/env")
        {
            return true;
        }
        false
    }
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
        // rm is now forbidden (merged from config.rs default_forbidden_commands)
        assert!(ctx.validate_command("rm -rf /").is_err());
        assert!(ctx.needs_approval("rm -rf /"));
        assert!(ctx.validate_command("echo hello").is_ok());
        // sh/bash are no longer forbidden — they require approval instead
        assert!(ctx.validate_command("bash -c 'echo hello'").is_ok());
        assert!(ctx.validate_command("sh -c 'ls'").is_ok());
    }

    #[test]
    fn test_forbidden_bypass_via_path_is_blocked() {
        let ctx = SafetyContext::default();
        // 绝对路径绕过
        assert!(ctx.validate_command("/usr/bin/rm -rf /").is_err());
        assert!(ctx.validate_command("/bin/sudo reboot").is_err());
        // 相对路径绕过
        assert!(ctx.validate_command("./rm -rf /").is_err());
        // env / command 包装器绕过
        assert!(ctx.validate_command("env rm -rf /").is_err());
        assert!(ctx.validate_command("command sudo reboot").is_err());
        // 合法命令仍可执行
        assert!(ctx.validate_command("/bin/echo hello").is_ok());
        assert!(ctx.validate_command("env echo hello").is_ok());
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
    fn test_auto_approve_tools() {
        let ctx = SafetyContext::default();
        // Auto-approved tools should not need approval
        assert!(!ctx.needs_approval("echo"));
        assert!(!ctx.needs_approval("file_read"));
        assert!(!ctx.needs_approval("list_directory"));
        assert!(!ctx.needs_approval("run_tests"));
        // Commands not in any list should not need approval
        assert!(!ctx.needs_approval("ls"));
        assert!(!ctx.needs_approval("cat"));
    }

    #[test]
    fn test_forbidden_and_approval_overlap() {
        let ctx = SafetyContext::default();
        // rm is both forbidden and in approval_required_tools;
        // forbidden takes precedence (validate_command rejects it)
        assert!(ctx.validate_command("rm -rf /").is_err());
        // but needs_approval also returns true
        assert!(ctx.needs_approval("rm -rf /"));
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

    /// M2/M3 regression: SafetyContext::default() must produce consistent sets
    /// across multiple calls (backed by LazyLock singletons).
    #[test]
    fn test_default_safety_context_is_consistent() {
        let a = SafetyContext::default();
        let b = SafetyContext::default();
        assert_eq!(a.forbidden_commands, b.forbidden_commands);
        assert_eq!(a.approval_required_tools, b.approval_required_tools);
        assert_eq!(a.auto_approve_tools, b.auto_approve_tools);
        // Mutating one clone must not affect the other
        let mut c = SafetyContext::default();
        c.forbidden_commands.insert("custom_cmd".into());
        let d = SafetyContext::default();
        assert!(!d.forbidden_commands.contains("custom_cmd"));
    }
}
