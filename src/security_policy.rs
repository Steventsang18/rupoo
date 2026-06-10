//! Security Policy module for fine-grained access control.
//!
//! This module implements a security policy system with:
//! - Default deny principle (everything is blocked by default)
//! - Explicit allow rules for specific operations
//! - Role-based access control (RBAC)
//! - Resource-level permissions
//! - Audit logging for security events

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use tracing::{debug, info};

use crate::error::{AgentError, AgentResult};

/// Security permission types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Permission {
    // System-level permissions
    SystemShutdown,
    SystemRestart,
    SystemConfig,

    // Tool permissions
    ToolExecute(String),
    ToolInstall(String),
    ToolUninstall(String),

    // File system permissions
    FileRead(String),
    FileWrite(String),
    FileDelete(String),
    FileExecute(String),

    // Network permissions
    NetworkAccess(String),
    NetworkBind(String),

    // Memory permissions
    MemoryRead,
    MemoryWrite,
    MemoryDelete,

    // Plan permissions
    PlanCreate,
    PlanExecute(String),
    PlanDelete(String),
    PlanView(String),

    // Skill permissions
    SkillExecute(String),
    SkillInstall(String),

    // Custom permissions
    Custom(String),
}

impl Permission {
    /// Check if this permission matches a pattern
    pub fn matches(&self, pattern: &Permission) -> bool {
        match (self, pattern) {
            (Permission::ToolExecute(a), Permission::ToolExecute(b)) => a == b || b == "*",
            (Permission::ToolInstall(a), Permission::ToolInstall(b)) => a == b || b == "*",
            (Permission::ToolUninstall(a), Permission::ToolUninstall(b)) => a == b || b == "*",
            (Permission::FileRead(a), Permission::FileRead(b)) => self.matches_path(a, b),
            (Permission::FileWrite(a), Permission::FileWrite(b)) => self.matches_path(a, b),
            (Permission::FileDelete(a), Permission::FileDelete(b)) => self.matches_path(a, b),
            (Permission::FileExecute(a), Permission::FileExecute(b)) => self.matches_path(a, b),
            (Permission::NetworkAccess(a), Permission::NetworkAccess(b)) => a == b || b == "*",
            (Permission::NetworkBind(a), Permission::NetworkBind(b)) => a == b || b == "*",
            (Permission::PlanExecute(a), Permission::PlanExecute(b)) => a == b || b == "*",
            (Permission::PlanDelete(a), Permission::PlanDelete(b)) => a == b || b == "*",
            (Permission::PlanView(a), Permission::PlanView(b)) => a == b || b == "*",
            (Permission::SkillExecute(a), Permission::SkillExecute(b)) => a == b || b == "*",
            (Permission::SkillInstall(a), Permission::SkillInstall(b)) => a == b || b == "*",
            (Permission::Custom(a), Permission::Custom(b)) => a == b || b == "*",
            (a, b) => a == b,
        }
    }

    fn matches_path(&self, path: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if path == pattern {
            return true;
        }
        path.starts_with(pattern)
            && (pattern.ends_with("/") || path.chars().nth(pattern.len()) == Some('/'))
    }
}

/// Security role
#[derive(Debug, Clone)]
pub struct Role {
    name: String,
    permissions: HashSet<Permission>,
    description: String,
}

impl Role {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            permissions: HashSet::new(),
            description: description.to_string(),
        }
    }

    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.permissions.iter().any(|p| permission.matches(p))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn permissions(&self) -> &HashSet<Permission> {
        &self.permissions
    }
}

/// Security policy with default-deny principle
#[derive(Debug)]
pub struct SecurityPolicy {
    roles: HashMap<String, Role>,
    user_roles: HashMap<String, HashSet<String>>,
    audit_log: RwLock<Vec<AuditEvent>>,
}

/// Audit event for security logging
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuditEvent {
    timestamp: chrono::DateTime<chrono::Utc>,
    user_id: String,
    permission: Permission,
    action: AuditAction,
    result: AuditResult,
    resource: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditAction {
    Check,
    Grant,
    Deny,
    Revoke,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditResult {
    Allowed,
    Denied,
    Failed,
}

impl SecurityPolicy {
    /// Create a new security policy with default-deny principle
    pub fn new() -> Self {
        let mut policy = Self {
            roles: HashMap::new(),
            user_roles: HashMap::new(),
            audit_log: RwLock::new(Vec::new()),
        };

        policy.init_default_roles();
        policy
    }

    /// Initialize default roles
    fn init_default_roles(&mut self) {
        let mut admin_role = Role::new("admin", "Full system administrator");
        admin_role.add_permission(Permission::SystemShutdown);
        admin_role.add_permission(Permission::SystemRestart);
        admin_role.add_permission(Permission::SystemConfig);
        admin_role.add_permission(Permission::ToolExecute("*".to_string()));
        admin_role.add_permission(Permission::ToolInstall("*".to_string()));
        admin_role.add_permission(Permission::ToolUninstall("*".to_string()));
        admin_role.add_permission(Permission::FileRead("*".to_string()));
        admin_role.add_permission(Permission::FileWrite("*".to_string()));
        admin_role.add_permission(Permission::FileDelete("*".to_string()));
        admin_role.add_permission(Permission::FileExecute("*".to_string()));
        admin_role.add_permission(Permission::NetworkAccess("*".to_string()));
        admin_role.add_permission(Permission::NetworkBind("*".to_string()));
        admin_role.add_permission(Permission::MemoryRead);
        admin_role.add_permission(Permission::MemoryWrite);
        admin_role.add_permission(Permission::MemoryDelete);
        admin_role.add_permission(Permission::PlanCreate);
        admin_role.add_permission(Permission::PlanExecute("*".to_string()));
        admin_role.add_permission(Permission::PlanDelete("*".to_string()));
        admin_role.add_permission(Permission::PlanView("*".to_string()));
        admin_role.add_permission(Permission::SkillExecute("*".to_string()));
        admin_role.add_permission(Permission::SkillInstall("*".to_string()));
        self.roles.insert("admin".to_string(), admin_role);

        let mut user_role = Role::new("user", "Regular user with basic permissions");
        user_role.add_permission(Permission::ToolExecute("*".to_string()));
        user_role.add_permission(Permission::FileRead("/home/".to_string()));
        user_role.add_permission(Permission::FileWrite("/home/".to_string()));
        user_role.add_permission(Permission::NetworkAccess("*".to_string()));
        user_role.add_permission(Permission::MemoryRead);
        user_role.add_permission(Permission::MemoryWrite);
        user_role.add_permission(Permission::PlanCreate);
        user_role.add_permission(Permission::PlanExecute("*".to_string()));
        user_role.add_permission(Permission::PlanView("*".to_string()));
        user_role.add_permission(Permission::SkillExecute("*".to_string()));
        self.roles.insert("user".to_string(), user_role);

        let mut guest_role = Role::new("guest", "Read-only guest access");
        guest_role.add_permission(Permission::FileRead("/public/".to_string()));
        guest_role.add_permission(Permission::MemoryRead);
        guest_role.add_permission(Permission::PlanView("*".to_string()));
        self.roles.insert("guest".to_string(), guest_role);
    }

    /// Register a new role
    pub fn register_role(&mut self, role: Role) -> AgentResult<()> {
        let role_name = role.name().to_string();
        if self.roles.contains_key(&role_name) {
            return Err(AgentError::Other(format!(
                "Role '{}' already exists",
                role_name
            )));
        }
        self.roles.insert(role_name.clone(), role);
        info!(role = role_name, "security role registered");
        Ok(())
    }

    /// Assign a role to a user
    pub fn assign_role(&mut self, user_id: &str, role_name: &str) -> AgentResult<()> {
        if !self.roles.contains_key(role_name) {
            return Err(AgentError::Other(format!("Role '{}' not found", role_name)));
        }
        self.user_roles
            .entry(user_id.to_string())
            .or_default()
            .insert(role_name.to_string());
        info!(user_id, role = role_name, "role assigned to user");
        Ok(())
    }

    /// Remove a role from a user
    pub fn remove_role(&mut self, user_id: &str, role_name: &str) -> AgentResult<()> {
        if let Some(roles) = self.user_roles.get_mut(user_id) {
            if roles.remove(role_name) {
                info!(user_id, role = role_name, "role removed from user");
                return Ok(());
            }
        }
        Err(AgentError::Other(format!(
            "User '{}' does not have role '{}'",
            user_id, role_name
        )))
    }

    /// Check if a user has a specific permission
    pub fn check_permission(&self, user_id: &str, permission: &Permission) -> bool {
        let result = self.has_permission(user_id, permission);
        let action = if result {
            AuditAction::Grant
        } else {
            AuditAction::Deny
        };
        let audit_result = if result {
            AuditResult::Allowed
        } else {
            AuditResult::Denied
        };

        self.log_audit_event(AuditEvent {
            timestamp: chrono::Utc::now(),
            user_id: user_id.to_string(),
            permission: permission.clone(),
            action,
            result: audit_result,
            resource: None,
            reason: None,
        });

        result
    }

    /// Internal permission check without audit logging
    fn has_permission(&self, user_id: &str, permission: &Permission) -> bool {
        // Default deny principle
        if !self.user_roles.contains_key(user_id) {
            debug!(user_id, "user not found, denying permission");
            return false;
        }

        let roles = self.user_roles.get(user_id).unwrap();
        for role_name in roles {
            if let Some(role) = self.roles.get(role_name) {
                if role.has_permission(permission) {
                    debug!(user_id, role = role_name, permission = ?permission, "permission granted");
                    return true;
                }
            }
        }

        debug!(user_id, permission = ?permission, "permission denied");
        false
    }

    /// Check permission with detailed result
    pub fn check_permission_detailed(
        &self,
        user_id: &str,
        permission: &Permission,
    ) -> PermissionCheckResult {
        let has_permission = self.has_permission(user_id, permission);
        let roles = self.user_roles.get(user_id).cloned().unwrap_or_default();

        PermissionCheckResult {
            allowed: has_permission,
            user_id: user_id.to_string(),
            permission: permission.clone(),
            roles,
            reason: if has_permission {
                "Permission granted by role".to_string()
            } else {
                "Permission denied (default deny)".to_string()
            },
        }
    }

    /// Log an audit event
    fn log_audit_event(&self, event: AuditEvent) {
        let mut log = self.audit_log.write().unwrap();
        log.push(event);

        let len = log.len();
        if len > 10000 {
            log.drain(0..len - 10000);
        }
    }

    /// Get recent audit events
    pub fn get_audit_events(&self, limit: usize) -> Vec<AuditEvent> {
        let log = self.audit_log.read().unwrap();
        let start = log.len().saturating_sub(limit);
        log[start..].to_vec()
    }

    /// Get all roles
    pub fn roles(&self) -> &HashMap<String, Role> {
        &self.roles
    }

    /// Get roles for a user
    pub fn get_user_roles(&self, user_id: &str) -> Option<&HashSet<String>> {
        self.user_roles.get(user_id)
    }

    /// Add a permission directly to a role
    pub fn add_role_permission(
        &mut self,
        role_name: &str,
        permission: Permission,
    ) -> AgentResult<()> {
        let role = self
            .roles
            .get_mut(role_name)
            .ok_or_else(|| AgentError::Other(format!("Role '{}' not found", role_name)))?;
        role.add_permission(permission.clone());
        info!(role = role_name, permission = ?permission, "permission added to role");
        Ok(())
    }
}

/// Result of a permission check
#[derive(Debug, Clone)]
pub struct PermissionCheckResult {
    pub allowed: bool,
    pub user_id: String,
    pub permission: Permission,
    pub roles: HashSet<String>,
    pub reason: String,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Operation risk assessment — integrates with tool_selector for safety grading
// ---------------------------------------------------------------------------

use crate::tool_selector::RiskLevel;

/// Assess the risk level of a tool operation based on its parameters,
/// combining static tool risk with parameter-sensitive analysis.
#[derive(Debug, Clone, Default)]
pub struct OperationRiskAssessor {
    /// Blocked paths that should never be accessed.
    blocked_paths: HashSet<String>,
    /// Blocked network hosts/domains.
    blocked_hosts: HashSet<String>,
    /// Whether to log all tool calls.
    audit_all: bool,
}

impl OperationRiskAssessor {
    /// Create with default blocked paths (system directories on each OS).
    pub fn new() -> Self {
        let mut assessor = Self {
            audit_all: true,
            ..Default::default()
        };

        // System directories that should never be directly modified
        #[cfg(target_os = "macos")]
        {
            assessor.blocked_paths.insert("/System".to_string());
            assessor.blocked_paths.insert("/Library/System".to_string());
        }
        #[cfg(target_os = "linux")]
        {
            assessor.blocked_paths.insert("/boot".to_string());
            assessor.blocked_paths.insert("/sys".to_string());
            assessor.blocked_paths.insert("/proc/kcore".to_string());
        }
        #[cfg(target_os = "windows")]
        {
            assessor
                .blocked_paths
                .insert("C:\\Windows\\System32".to_string());
        }

        assessor
    }

    /// Assess a shell command for risk escalation.
    /// Returns (risk_level, reason).
    pub fn assess_shell_command(&self, command: &str) -> (RiskLevel, Option<String>) {
        let lower = command.to_lowercase().trim().to_string();

        // Critical: commands that can destroy the system
        let critical_patterns = [
            "rm -rf /",
            "mkfs.",
            "dd if=",
            ":(){ :|:& };:",
            "> /dev/sda",
            "chmod 777 /",
            "chown -R root:root /",
            "> /dev/null 2>&1 &",
        ];
        for pat in &critical_patterns {
            if lower.contains(pat) {
                return (
                    RiskLevel::Critical,
                    Some(format!("Command matches critical pattern: {pat}")),
                );
            }
        }

        // High: commands that modify system state
        let high_patterns = [
            "sudo ",
            "su ",
            "rm ",
            "chmod ",
            "chown ",
            "mount ",
            "umount ",
            "systemctl ",
            "service ",
            "kill -9",
            "pkill ",
            "docker rm",
        ];
        for pat in &high_patterns {
            if lower.contains(pat) {
                return (
                    RiskLevel::High,
                    Some(format!("Privileged or destructive command: {pat}")),
                );
            }
        }

        // Medium: commands that install or modify software
        let medium_patterns = [
            "pip install",
            "npm install -g",
            "cargo install",
            "gem install",
            "brew install",
            "apt-get install",
            "yum install",
            "pacman -S",
        ];
        for pat in &medium_patterns {
            if lower.contains(pat) {
                return (
                    RiskLevel::Medium,
                    Some(format!("Installation command: {pat}")),
                );
            }
        }

        // Default: low risk
        (RiskLevel::Low, None)
    }

    /// Assess file operation risk based on path.
    pub fn assess_file_path(&self, path: &str, is_write: bool) -> (RiskLevel, Option<String>) {
        let normalized = path.trim();

        // Check against blocked paths
        for blocked in &self.blocked_paths {
            if normalized.starts_with(blocked) {
                return (
                    RiskLevel::Critical,
                    Some(format!("Path '{}' is in blocked system directory", blocked)),
                );
            }
        }

        // Sensitive config files
        let sensitive_paths = [".env", ".gitconfig", "id_rsa", "credentials", ".aws/"];
        for sensitive in &sensitive_paths {
            if normalized.contains(sensitive) {
                if is_write {
                    return (
                        RiskLevel::High,
                        Some("Modifying sensitive config file".to_string()),
                    );
                }
                return (
                    RiskLevel::Medium,
                    Some("Reading sensitive config file".to_string()),
                );
            }
        }

        if is_write {
            (RiskLevel::Medium, None)
        } else {
            (RiskLevel::Safe, None)
        }
    }

    /// Assess network URL risk (SSRF, metadata services, etc.).
    pub fn assess_network_url(&self, url: &str) -> (RiskLevel, Option<String>) {
        // Metadata service IPs
        let metadata_ips = [
            "169.254.169.254",
            "100.100.100.200",
            "metadata.google.internal",
        ];
        for ip in &metadata_ips {
            if url.contains(ip) {
                return (
                    RiskLevel::Critical,
                    Some("Request to cloud metadata service".to_string()),
                );
            }
        }

        // Internal IP ranges
        let internal_prefixes = [
            "10.", "172.16.", "172.17.", "172.18.", "172.19.", "172.20.", "172.21.", "172.22.",
            "172.23.", "172.24.", "172.25.", "172.26.", "172.27.", "172.28.", "172.29.", "172.30.",
            "172.31.", "192.168.",
        ];
        for prefix in &internal_prefixes {
            if url.contains(prefix) {
                return (
                    RiskLevel::High,
                    Some("Request to internal/private IP range".to_string()),
                );
            }
        }

        // Check blocked hosts
        for host in &self.blocked_hosts {
            if url.contains(host.as_str()) {
                return (
                    RiskLevel::Critical,
                    Some(format!("Request to blocked host: {host}")),
                );
            }
        }

        (RiskLevel::Low, None)
    }

    /// Block a host from network access.
    pub fn block_host(&mut self, host: &str) {
        self.blocked_hosts.insert(host.to_string());
        info!(host, "host blocked from network access");
    }

    /// Block a path from file access.
    pub fn block_path(&mut self, path: &str) {
        self.blocked_paths.insert(path.to_string());
        info!(path, "path blocked from file access");
    }

    /// Enable or disable full audit logging.
    pub fn set_audit_all(&mut self, enabled: bool) {
        self.audit_all = enabled;
    }

    /// Whether auditing is enabled.
    pub fn is_auditing(&self) -> bool {
        self.audit_all
    }
}

// ---------------------------------------------------------------------------
// Tool execution guard — combines selector metadata with safety policy
// ---------------------------------------------------------------------------

/// Result of a tool safety check.
#[derive(Debug, Clone)]
pub struct ToolSafetyCheck {
    /// Whether the tool is allowed to execute.
    pub allowed: bool,
    /// Whether user approval is required.
    pub requires_approval: bool,
    /// Risk level of the operation.
    pub risk_level: RiskLevel,
    /// Warning message if any concerns.
    pub warning: Option<String>,
    /// Block reason if not allowed.
    pub block_reason: Option<String>,
}

impl ToolSafetyCheck {
    pub fn allowed() -> Self {
        Self {
            allowed: true,
            requires_approval: false,
            risk_level: RiskLevel::Safe,
            warning: None,
            block_reason: None,
        }
    }

    pub fn requires_approval(risk: RiskLevel, reason: Option<String>) -> Self {
        Self {
            allowed: true,
            requires_approval: true,
            risk_level: risk,
            warning: reason,
            block_reason: None,
        }
    }

    pub fn blocked(reason: String) -> Self {
        Self {
            allowed: false,
            requires_approval: false,
            risk_level: RiskLevel::Critical,
            warning: Some(reason.clone()),
            block_reason: Some(reason),
        }
    }
}

/// Perform a comprehensive safety check on a tool call.
///
/// Combines static tool metadata with dynamic parameter analysis.
pub fn check_tool_safety(
    tool_name: &str,
    params: &serde_json::Value,
    assessor: &OperationRiskAssessor,
) -> ToolSafetyCheck {
    let lower_name = tool_name.to_lowercase();

    match lower_name.as_str() {
        "shell_exec" | "sh" | "bash" => {
            let cmd = params.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let (risk, reason) = assessor.assess_shell_command(cmd);
            match risk {
                RiskLevel::Critical => ToolSafetyCheck::blocked(
                    reason.unwrap_or_else(|| "Critical risk shell command".to_string()),
                ),
                RiskLevel::High => ToolSafetyCheck::requires_approval(risk, reason),
                _ => {
                    let mut check = ToolSafetyCheck::allowed();
                    check.risk_level = risk;
                    check.warning = reason;
                    check
                }
            }
        }

        "file_write" | "write" | "save" => {
            let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let (risk, reason) = assessor.assess_file_path(path, true);
            match risk {
                RiskLevel::Critical => ToolSafetyCheck::blocked(
                    reason.unwrap_or_else(|| "Critical risk file path".to_string()),
                ),
                RiskLevel::High => ToolSafetyCheck::requires_approval(risk, reason),
                _ => {
                    let mut check = ToolSafetyCheck::allowed();
                    check.risk_level = risk;
                    check.warning = reason;
                    check
                }
            }
        }

        "file_read" | "read" | "open" => {
            let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let (risk, reason) = assessor.assess_file_path(path, false);
            match risk {
                RiskLevel::Critical => ToolSafetyCheck::blocked(
                    reason.unwrap_or_else(|| "Critical risk file path".to_string()),
                ),
                RiskLevel::High => ToolSafetyCheck::requires_approval(risk, reason),
                _ => {
                    let mut check = ToolSafetyCheck::allowed();
                    check.risk_level = risk;
                    check.warning = reason;
                    check
                }
            }
        }

        "delete_file" | "rm" | "remove" | "delete" => {
            let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let (risk, reason) = assessor.assess_file_path(path, true);
            match risk {
                RiskLevel::Critical => ToolSafetyCheck::blocked(
                    reason.unwrap_or_else(|| "Cannot delete critical system file".to_string()),
                ),
                _ => ToolSafetyCheck::requires_approval(
                    RiskLevel::High,
                    Some(format!("File deletion: {}", path)),
                ),
            }
        }

        "http_get" | "http_post" | "http_request" | "web_request" => {
            let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let (risk, reason) = assessor.assess_network_url(url);
            match risk {
                RiskLevel::Critical => ToolSafetyCheck::blocked(
                    reason.unwrap_or_else(|| "Blocked network request".to_string()),
                ),
                RiskLevel::High => ToolSafetyCheck::requires_approval(risk, reason),
                _ => {
                    let mut check = ToolSafetyCheck::allowed();
                    check.risk_level = risk;
                    check.warning = reason;
                    check
                }
            }
        }

        _ => ToolSafetyCheck::allowed(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_deny_principle() {
        let policy = SecurityPolicy::new();

        assert!(!policy.check_permission("unknown_user", &Permission::SystemShutdown));
        assert!(!policy.check_permission(
            "unknown_user",
            &Permission::FileRead("/etc/passwd".to_string())
        ));
    }

    #[test]
    fn test_admin_role_has_all_permissions() {
        let mut policy = SecurityPolicy::new();
        policy.assign_role("admin_user", "admin").unwrap();

        assert!(policy.check_permission("admin_user", &Permission::SystemShutdown));
        assert!(policy.check_permission(
            "admin_user",
            &Permission::FileRead("/etc/passwd".to_string())
        ));
        assert!(policy.check_permission("admin_user", &Permission::NetworkAccess("*".to_string())));
    }

    #[test]
    fn test_user_role_permissions() {
        let mut policy = SecurityPolicy::new();
        policy.assign_role("regular_user", "user").unwrap();

        assert!(
            policy.check_permission("regular_user", &Permission::ToolExecute("bash".to_string()))
        );
        assert!(policy.check_permission(
            "regular_user",
            &Permission::FileRead("/home/user/doc.txt".to_string())
        ));
        assert!(!policy.check_permission("regular_user", &Permission::SystemShutdown));
    }

    #[test]
    fn test_guest_role_permissions() {
        let mut policy = SecurityPolicy::new();
        policy.assign_role("guest_user", "guest").unwrap();

        assert!(policy.check_permission(
            "guest_user",
            &Permission::FileRead("/public/readme.txt".to_string())
        ));
        assert!(!policy.check_permission(
            "guest_user",
            &Permission::FileWrite("/public/test.txt".to_string())
        ));
        assert!(!policy.check_permission("guest_user", &Permission::ToolExecute("rm".to_string())));
    }

    #[test]
    fn test_path_pattern_matching() {
        let mut policy = SecurityPolicy::new();
        policy.assign_role("user1", "user").unwrap();

        assert!(policy.check_permission(
            "user1",
            &Permission::FileRead("/home/user/doc.txt".to_string())
        ));
        assert!(!policy.check_permission("user1", &Permission::FileRead("/etc/passwd".to_string())));
    }

    #[test]
    fn test_custom_role() {
        let mut policy = SecurityPolicy::new();

        let mut custom_role = Role::new("developer", "Developer role with limited permissions");
        custom_role.add_permission(Permission::ToolExecute("git".to_string()));
        custom_role.add_permission(Permission::FileRead("/projects/".to_string()));
        custom_role.add_permission(Permission::FileWrite("/projects/".to_string()));
        custom_role.add_permission(Permission::NetworkAccess("github.com".to_string()));

        policy.register_role(custom_role).unwrap();
        policy.assign_role("dev_user", "developer").unwrap();

        assert!(policy.check_permission("dev_user", &Permission::ToolExecute("git".to_string())));
        assert!(policy.check_permission(
            "dev_user",
            &Permission::FileRead("/projects/myapp/main.rs".to_string())
        ));
        assert!(!policy.check_permission("dev_user", &Permission::ToolExecute("rm".to_string())));
        assert!(!policy.check_permission(
            "dev_user",
            &Permission::NetworkAccess("evil.com".to_string())
        ));
    }

    #[test]
    fn test_audit_logging() {
        let mut policy = SecurityPolicy::new();
        policy.assign_role("test_user", "user").unwrap();

        policy.check_permission("test_user", &Permission::SystemShutdown);
        policy.check_permission("test_user", &Permission::ToolExecute("ls".to_string()));

        let events = policy.get_audit_events(10);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_permission_check_detailed() {
        let mut policy = SecurityPolicy::new();
        policy.assign_role("test_user", "user").unwrap();

        let result = policy
            .check_permission_detailed("test_user", &Permission::ToolExecute("ls".to_string()));
        assert!(result.allowed);
        assert_eq!(result.user_id, "test_user");
        assert!(result.roles.contains("user"));

        let result = policy.check_permission_detailed("test_user", &Permission::SystemShutdown);
        assert!(!result.allowed);
    }

    // --- OperationRiskAssessor tests ---

    #[test]
    fn test_assess_shell_command_critical() {
        let assessor = OperationRiskAssessor::new();
        let (risk, reason) = assessor.assess_shell_command("rm -rf /");
        assert_eq!(risk, RiskLevel::Critical);
        assert!(reason.is_some());
    }

    #[test]
    fn test_assess_shell_command_high() {
        let assessor = OperationRiskAssessor::new();
        let (risk, _) = assessor.assess_shell_command("sudo systemctl restart");
        assert_eq!(risk, RiskLevel::High);
    }

    #[test]
    fn test_assess_shell_command_medium() {
        let assessor = OperationRiskAssessor::new();
        let (risk, _) = assessor.assess_shell_command("pip install requests");
        assert_eq!(risk, RiskLevel::Medium);
    }

    #[test]
    fn test_assess_shell_command_low() {
        let assessor = OperationRiskAssessor::new();
        let (risk, _) = assessor.assess_shell_command("ls -la");
        assert_eq!(risk, RiskLevel::Low);
    }

    #[test]
    fn test_assess_file_path_sensitive() {
        let assessor = OperationRiskAssessor::new();
        let (risk, reason) = assessor.assess_file_path(".env", true);
        assert_eq!(risk, RiskLevel::High);
        assert!(reason.is_some());
    }

    #[test]
    fn test_assess_file_path_normal() {
        let assessor = OperationRiskAssessor::new();
        let (risk, _) = assessor.assess_file_path("src/main.rs", false);
        assert_eq!(risk, RiskLevel::Safe);
    }

    #[test]
    fn test_assess_network_metadata() {
        let assessor = OperationRiskAssessor::new();
        let (risk, _) = assessor.assess_network_url("http://169.254.169.254/latest/meta-data/");
        assert_eq!(risk, RiskLevel::Critical);
    }

    #[test]
    fn test_assess_network_internal() {
        let assessor = OperationRiskAssessor::new();
        let (risk, _) = assessor.assess_network_url("http://192.168.1.1/admin");
        assert_eq!(risk, RiskLevel::High);
    }

    #[test]
    fn test_assess_network_normal() {
        let assessor = OperationRiskAssessor::new();
        let (risk, _) = assessor.assess_network_url("https://api.github.com");
        assert_eq!(risk, RiskLevel::Low);
    }

    #[test]
    fn test_block_host() {
        let mut assessor = OperationRiskAssessor::new();
        assessor.block_host("evil.com");
        let (risk, _) = assessor.assess_network_url("http://evil.com/phishing");
        assert_eq!(risk, RiskLevel::Critical);
    }

    #[test]
    fn test_check_tool_safety_critical_shell() {
        let assessor = OperationRiskAssessor::new();
        let params = serde_json::json!({"command": "rm -rf /"});
        let result = check_tool_safety("shell_exec", &params, &assessor);
        assert!(!result.allowed);
        assert!(result.block_reason.is_some());
    }

    #[test]
    fn test_check_tool_safety_safe_read() {
        let assessor = OperationRiskAssessor::new();
        let params = serde_json::json!({"path": "src/main.rs"});
        let result = check_tool_safety("file_read", &params, &assessor);
        assert!(result.allowed);
        assert!(!result.requires_approval);
    }

    #[test]
    fn test_check_tool_safety_delete_requires_approval() {
        let assessor = OperationRiskAssessor::new();
        let params = serde_json::json!({"path": "test.txt"});
        let result = check_tool_safety("delete_file", &params, &assessor);
        assert!(result.allowed);
        assert!(result.requires_approval);
    }
}
