//! Approval policy abstraction — configurable tool approval strategies.
//!
//! Three built-in policies:
//! - `AlwaysAskPolicy` — every operation requires approval
//! - `DangerousOnlyPolicy` — only dangerous operations require approval (default)
//! - `AllowAllPolicy` — all operations auto-approved (for headless/CI mode)
//!
//! Custom policies can be created by implementing the `ApprovalPolicy` trait.
//! Policy selection is configured in `~/.rupoo/config.toml` under `[safety]`.

use std::collections::HashSet;

/// Trait for approval policy — determines if a tool call needs human approval.
pub trait ApprovalPolicy: Send + Sync {
    /// Check if a tool call with the given name and parameters requires approval.
    fn needs_approval(&self, tool_name: &str, params: &serde_json::Value) -> bool;

    /// Human-readable name for this policy.
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// Built-in policies
// ---------------------------------------------------------------------------

/// All operations require explicit user approval.
pub struct AlwaysAskPolicy;

impl ApprovalPolicy for AlwaysAskPolicy {
    fn needs_approval(&self, _tool_name: &str, _params: &serde_json::Value) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "always_ask"
    }
}

/// Only dangerous operations require approval (default).
///
/// Dangerous = shell commands, file writes, network POSTs, deletions.
/// Safe = file reads, directory listings, web searches, echo.
pub struct DangerousOnlyPolicy {
    /// Tools that are always auto-approved regardless of name.
    auto_approve: HashSet<String>,
    /// Command prefixes that require approval.
    dangerous_commands: HashSet<String>,
}

impl DangerousOnlyPolicy {
    pub fn new() -> Self {
        Self {
            auto_approve: DEFAULT_AUTO_APPROVE.iter().map(|s| s.to_string()).collect(),
            dangerous_commands: DANGEROUS_COMMANDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Create with custom auto-approve and dangerous-command lists.
    pub fn with_lists(
        auto_approve: Vec<String>,
        dangerous_commands: Vec<String>,
    ) -> Self {
        Self {
            auto_approve: auto_approve.into_iter().collect(),
            dangerous_commands: dangerous_commands.into_iter().collect(),
        }
    }
}

impl Default for DangerousOnlyPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovalPolicy for DangerousOnlyPolicy {
    fn needs_approval(&self, tool_name: &str, params: &serde_json::Value) -> bool {
        let lower = tool_name.to_lowercase();
        let base = lower.split_whitespace().next().unwrap_or(&lower);

        // Always auto-approve known-safe tools
        if self.auto_approve.contains(base) {
            return false;
        }

        // Check if it's a known dangerous command
        if self.dangerous_commands.contains(base) {
            return true;
        }

        // Shell exec needs approval (can run arbitrary commands)
        if base == "shell_exec" {
            // Extract the actual command from params and check
            if let Some(cmd) = params.get("command").and_then(|v| v.as_str()) {
                let cmd_base = cmd.split_whitespace().next().unwrap_or(cmd);
                return self.dangerous_commands.contains(&cmd_base.to_lowercase())
                    || cmd.starts_with("/bin/")
                    || cmd.starts_with("/usr/bin/env");
            }
            return true; // Unknown shell command → require approval
        }

        // File write needs approval
        if base == "file_write" {
            return true;
        }

        // HTTP POST/DELETE needs approval
        if base == "http_post" || base == "http_delete" {
            return true;
        }

        // Default: don't require approval
        false
    }

    fn name(&self) -> &'static str {
        "dangerous_only"
    }
}

/// All operations are auto-approved. Use in headless/CI mode only.
///
/// ⚠️ All actions are logged for audit trail.
pub struct AllowAllPolicy;

impl ApprovalPolicy for AllowAllPolicy {
    fn needs_approval(&self, _tool_name: &str, _params: &serde_json::Value) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "allow_all"
    }
}

// ---------------------------------------------------------------------------
// Policy factory
// ---------------------------------------------------------------------------

/// Create an approval policy from a config name.
pub fn policy_from_name(name: &str) -> Box<dyn ApprovalPolicy> {
    match name {
        "always_ask" => Box::new(AlwaysAskPolicy),
        "allow_all" => Box::new(AllowAllPolicy),
        _ => Box::new(DangerousOnlyPolicy::new()), // default
    }
}

/// Create an approval policy from config with custom lists.
pub fn policy_from_config(
    name: &str,
    auto_approve: Vec<String>,
    dangerous_commands: Vec<String>,
) -> Box<dyn ApprovalPolicy> {
    match name {
        "always_ask" => Box::new(AlwaysAskPolicy),
        "allow_all" => Box::new(AllowAllPolicy),
        _ => {
            if auto_approve.is_empty() && dangerous_commands.is_empty() {
                Box::new(DangerousOnlyPolicy::new())
            } else {
                Box::new(DangerousOnlyPolicy::with_lists(auto_approve, dangerous_commands))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Default lists
// ---------------------------------------------------------------------------

const DEFAULT_AUTO_APPROVE: &[&str] = &[
    "echo",
    "file_read",
    "list_directory",
    "run_tests",
    "check_output",
    "diff_check",
    "web_search",
];

const DANGEROUS_COMMANDS: &[&str] = &[
    "sudo", "su", "passwd", "chown", "chmod", "chattr",
    "rm", "mkfs", "fdisk", "dd", "format",
    "shutdown", "reboot", "halt", "poweroff",
    "kill", "killall", "pkill",
    "iptables", "ufw",
    "mount", "umount",
    "bash", "sh", "zsh", "dash", "ksh", "csh", "fish",
    "python", "python3", "perl", "ruby", "node", "lua",
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_always_ask() {
        let policy = AlwaysAskPolicy;
        assert!(policy.needs_approval("echo", &serde_json::json!({})));
        assert!(policy.needs_approval("file_read", &serde_json::json!({})));
    }

    #[test]
    fn test_allow_all() {
        let policy = AllowAllPolicy;
        assert!(!policy.needs_approval("rm", &serde_json::json!({})));
        assert!(!policy.needs_approval("sudo", &serde_json::json!({})));
    }

    #[test]
    fn test_dangerous_only_safe() {
        let policy = DangerousOnlyPolicy::new();
        assert!(!policy.needs_approval("echo", &serde_json::json!({})));
        assert!(!policy.needs_approval("file_read", &serde_json::json!({})));
        assert!(!policy.needs_approval("list_directory", &serde_json::json!({})));
        assert!(!policy.needs_approval("web_search", &serde_json::json!({})));
    }

    #[test]
    fn test_dangerous_only_dangerous() {
        let policy = DangerousOnlyPolicy::new();
        assert!(policy.needs_approval("shell_exec", &serde_json::json!({"command": "sudo rm -rf /"})));
        assert!(policy.needs_approval("file_write", &serde_json::json!({})));
        assert!(policy.needs_approval("http_post", &serde_json::json!({})));
    }

    #[test]
    fn test_dangerous_only_shell_safe() {
        let policy = DangerousOnlyPolicy::new();
        // ls command is not in dangerous list
        assert!(!policy.needs_approval("shell_exec", &serde_json::json!({"command": "ls -la"})));
    }

    #[test]
    fn test_policy_from_name() {
        let p1 = policy_from_name("always_ask");
        assert_eq!(p1.name(), "always_ask");

        let p2 = policy_from_name("allow_all");
        assert_eq!(p2.name(), "allow_all");

        let p3 = policy_from_name("dangerous_only");
        assert_eq!(p3.name(), "dangerous_only");

        let p4 = policy_from_name("unknown");
        assert_eq!(p4.name(), "dangerous_only");
    }

    #[test]
    fn test_custom_lists() {
        let policy = DangerousOnlyPolicy::with_lists(
            vec!["custom_safe".into()],
            vec!["custom_dangerous".into()],
        );
        assert!(!policy.needs_approval("custom_safe", &serde_json::json!({})));
        assert!(policy.needs_approval("custom_dangerous", &serde_json::json!({})));
    }
}
