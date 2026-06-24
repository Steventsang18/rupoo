#[cfg(test)]
mod tests {
    use crate::supervisor::{Action, ExecutionMeta};
    use crate::supervisor::audit_logger::{AuditEvent, AuditEventType, AuditResult};

    #[test]
    fn test_action_construction() {
        let action = Action::new("execute_command", "rm -rf /tmp")
            .with_payload(serde_json::json!({"command": "rm"}));
        assert_eq!(action.action_type, "execute_command");
        assert_eq!(action.description, "rm -rf /tmp");
        assert_eq!(action.payload["command"], "rm");
    }

    #[test]
    fn test_execution_meta_confidence() {
        let meta = ExecutionMeta::with_confidence(0.85);
        assert_eq!(meta.confidence, Some(0.85));
    }

    #[test]
    fn test_audit_event_new() {
        let event = AuditEvent::new(
            AuditEventType::ComplianceCheck,
            "supervisor",
            &serde_json::json!({"tool": "bash"}),
        );
        assert_eq!(event.event_type, AuditEventType::ComplianceCheck);
        assert_eq!(event.result, AuditResult::Passed);
    }

    #[test]
    fn test_audit_event_new_blocked() {
        let event = AuditEvent::new_blocked(
            AuditEventType::ComplianceCheck,
            "supervisor",
            "forbidden command: sudo",
        );
        assert_eq!(event.result, AuditResult::Blocked);
        assert_eq!(event.detail["reason"], "forbidden command: sudo");
    }
}
