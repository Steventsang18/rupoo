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
}
