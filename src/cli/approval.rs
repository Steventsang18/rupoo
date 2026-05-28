//! Approval handling — tool execution after user approval

use tracing::warn;

use super::{AgentToTui, ChatMessage};
use super::bridge::AgentUiBridge;

// Trait to group approval-related methods
pub(super) trait ApprovalExt {
    /// Execute an approved tool: mark step Running, execute, record result.
    async fn execute_approved_tool(&self, plan: &mut rupoo::task::Plan, step_index: usize);
    /// Handle user approval (ApproveTool or ApproveAll).
    async fn handle_approval(&mut self);
    /// Handle user denial (DenyTool).
    async fn handle_denial(&mut self);
}

impl ApprovalExt for AgentUiBridge {
    async fn execute_approved_tool(&self, plan: &mut rupoo::task::Plan, step_index: usize) {
        let pid = plan.id.clone();
        let (tool_name, params) = if let Some(rupoo::task::Step::ToolCall { ref tool_name, ref params, .. }) =
            plan.steps.get(step_index)
        {
            (tool_name.clone(), params.clone())
        } else {
            return;
        };

        // Mark step Running
        if let Err(e) = self
            .repo
            .update_step_progress(&pid, step_index, rupoo::task::StepStatus::Running)
            .await
        {
            warn!(error = %e, "failed to mark step running");
        }

        // Execute the tool directly (bypass needs_approval check)
        let mcp_result = self.tool_executor.execute_tool(&tool_name, params).await;

        match mcp_result {
            Ok(mcp) => {
                if let Some(step) = plan.steps.get_mut(step_index) {
                    step.set_status(rupoo::task::StepStatus::Completed);
                    if let rupoo::task::Step::ToolCall { ref mut result, .. } = step {
                        *result = Some(serde_json::json!({
                            "success": mcp.success,
                            "content": mcp.content,
                        }));
                    }
                }
                let _ = self
                    .repo
                    .record_step_completion(
                        &pid,
                        step_index,
                        rupoo::task::StepStatus::Completed,
                        Some(mcp.content.clone()),
                    )
                    .await;
                plan.current_step_index = step_index + 1;
                plan.updated_at = chrono::Utc::now();
            }
            Err(e) => {
                if let Some(step) = plan.steps.get_mut(step_index) {
                    step.set_status(rupoo::task::StepStatus::Failed);
                }
                let _ = self
                    .repo
                    .record_step_completion(
                        &pid,
                        step_index,
                        rupoo::task::StepStatus::Failed,
                        Some(format!("execution error: {e}")),
                    )
                    .await;
                plan.updated_at = chrono::Utc::now();
            }
        }
    }

    /// Handle user approval (ApproveTool or ApproveAll).
    /// Shared logic for both ApproveTool and ApproveAll commands.
    async fn handle_approval(&mut self) {
        let pending = self.pending_plan.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let step_idx = *self.pending_step_index.lock().unwrap_or_else(|e| e.into_inner());
        if let (Some(mut plan), Some(step_index)) = (pending, step_idx) {
            self.execute_approved_tool(&mut plan, step_index).await;
            self.run_plan(&mut plan).await;
        } else {
            let _ = self.ui_tx.send(AgentToTui::Message(
                ChatMessage::assistant("No pending tool to approve.".to_string()),
            ));
        }
        let _ = self.ui_tx.send(AgentToTui::Idle);
    }

    /// Handle user denial (DenyTool).
    /// Mark the step as Failed, do not execute.
    async fn handle_denial(&mut self) {
        // User denied the tool call — mark the step as Failed, do not execute.
        let pending = self.pending_plan.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let step_idx = *self.pending_step_index.lock().unwrap_or_else(|e| e.into_inner());
        if let (Some(mut plan), Some(step_index)) = (pending, step_idx) {
            let pid = plan.id.clone();

            // Mark step as Failed (not Completed — user explicitly denied it)
            if let Some(step) = plan.steps.get_mut(step_index) {
                step.set_status(rupoo::task::StepStatus::Failed);
            }
            let _ = self
                .repo
                .record_step_completion(
                    &pid,
                    step_index,
                    rupoo::task::StepStatus::Failed,
                    Some("denied by user".to_string()),
                )
                .await;

            plan.updated_at = chrono::Utc::now();

            let _ = self.ui_tx.send(AgentToTui::Message(
                ChatMessage::assistant("Tool call denied by user.".to_string()),
            ));

            // Continue running the plan — it will handle the failure
            // gracefully (agent decides how to proceed with a failed step).
            self.run_plan(&mut plan).await;
        }
        let _ = self.ui_tx.send(AgentToTui::Idle);
    }
}
