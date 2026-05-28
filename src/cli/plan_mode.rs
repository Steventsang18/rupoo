//! Plan Mode — generate and execute step-by-step plans

use tracing::warn;

use super::{AgentToTui, ChatMessage, PendingTool};
use super::bridge::AgentUiBridge;

impl AgentUiBridge {
    /// Handle Plan Mode: generate plan from task and execute step by step.
    pub(super) async fn handle_plan_mode(&mut self, task: &str) {
        // Check if LLM is configured
        if !self.agent.has_llm() {
            let _ = self.ui_tx.send(AgentToTui::Message(
                ChatMessage::error("LLM not configured. Please set up your API key first.".to_string()),
            ));
            let _ = self.ui_tx.send(AgentToTui::Idle);
            return;
        }

        let _ = self.ui_tx.send(AgentToTui::Thinking);
        let _ = self.ui_tx.send(AgentToTui::Message(
            ChatMessage::system(format!("Generating plan for: {}", task)),
        ));

        // Get the LLM gateway to generate plan
        let gateway = match self.agent.llm_gateway_ref() {
            Some(g) => g,
            None => {
                let _ = self.ui_tx.send(AgentToTui::Message(
                    ChatMessage::error("LLM gateway not available".to_string()),
                ));
                let _ = self.ui_tx.send(AgentToTui::Idle);
                return;
            }
        };

        match gateway.generate_plan(task).await {
            Ok(steps) => {
                let total = steps.len();
                let _ = self.ui_tx.send(AgentToTui::Message(
                    ChatMessage::system(format!("Generated plan with {} steps", total)),
                ));

                // Convert StepSpec to Plan steps
                let plan_steps: Vec<rupoo::task::Step> = steps.into_iter().map(|spec| {
                    match spec.step_type.as_str() {
                        "think" => rupoo::task::think_step(&spec.prompt),
                        "exec" => rupoo::task::exec_step(
                            if spec.tool_name.is_empty() { "bash" } else { &spec.tool_name },
                            vec![],
                            None,
                        ),
                        "finish" => rupoo::task::finish_step(&spec.summary),
                        "wait_for_input" => rupoo::task::wait_for_input_step(&spec.prompt),
                        _ => rupoo::task::think_step(&spec.instruction),
                    }
                }).collect();

                // Create and save the plan
                let label: String = task.chars().take(40).collect();
                let plan = rupoo::task::Plan::new(&label, plan_steps);

                if let Err(e) = self.repo.save_plan(&plan).await {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::error(format!("Failed to save plan: {}", e)),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    return;
                }

                // Run the plan
                match self.agent.resume(&plan.id).await {
                    Ok(Some(mut plan)) => {
                        self.run_plan(&mut plan).await;
                    }
                    Ok(None) => {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant("Plan already completed".to_string()),
                        ));
                        let _ = self.ui_tx.send(AgentToTui::Idle);
                    }
                    Err(e) => {
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::error(format!("Plan error: {}", e)),
                        ));
                        let _ = self.ui_tx.send(AgentToTui::Idle);
                    }
                }
            }
            Err(e) => {
                let _ = self.ui_tx.send(AgentToTui::Message(
                    ChatMessage::error(format!("Failed to generate plan: {}", e)),
                ));
                let _ = self.ui_tx.send(AgentToTui::Idle);
            }
        }
    }

    pub(super) async fn run_plan(&self, plan: &mut rupoo::task::Plan) {
        *self.pending_plan.lock().unwrap() = Some(plan.clone());
        loop {
            // Send step progress update
            let step_name = plan.steps.get(plan.current_step_index)
                .map(|s| format!("{:?}", s))
                .unwrap_or_else(|| "unknown".to_string());
            let _ = self.ui_tx.send(AgentToTui::StepProgress {
                step_index: plan.current_step_index,
                total: plan.steps.len(),
                step_name,
            });

            match self.agent.run_next_step(plan).await {
                Ok(rupoo::agent::StepOutcome::Advanced) => {
                    // Send token update BEFORE last-step check so it's always emitted
                    self.send_token_update();
                    if plan.current_step_index >= plan.steps.len() {
                        // Send the final output before going idle
                        let output = self.extract_output(plan);
                        let _ = self.ui_tx.send(AgentToTui::Message(
                            ChatMessage::assistant(output),
                        ));
                        let _ = self.ui_tx.send(AgentToTui::Idle);
                        *self.pending_plan.lock().unwrap() = None;
                        *self.pending_step_index.lock().unwrap() = None;
                        break;
                    }
                }
                Ok(rupoo::agent::StepOutcome::Finished) => {
                    self.send_token_update();
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(self.extract_output(plan)),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
                Ok(rupoo::agent::StepOutcome::Failed(e)) => {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(format!("Failed: {e}")),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
                Ok(rupoo::agent::StepOutcome::WaitingForInput(p)) => {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(format!("Input needed: {p}")),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
                Ok(rupoo::agent::StepOutcome::RequiresApproval { ref tool_name, ref params, step_index }) => {
                    if self.approve_all {
                        // Auto-approve: execute based on step type.
                        let p = params.clone();
                        let tn = tool_name.clone();
                        if let Err(e) = self.repo.update_step_progress(
                            &plan.id, step_index, rupoo::task::StepStatus::Running,
                        ).await {
                            warn!(error = %e, "failed to mark step running");
                        }

                        // Determine execution method based on step type
                        let result = if let Some(rupoo::task::Step::Exec { command, args, timeout_secs, .. }) = plan.steps.get(step_index) {
                            // Exec step: run via terminal executor
                            let cmd = command.clone();
                            let a = args.clone();
                            let t = *timeout_secs;
                            rupoo::tools::terminal::execute_command(
                                &cmd, &a, t, &self.agent.safety_ctx,
                            ).await.map_err(|e| e.to_string())
                        } else {
                            // ToolCall step: run via MCP executor
                            self.tool_executor.execute_tool(&tn, p).await
                                .map(|mcp| mcp.content)
                                .map_err(|e| e.to_string())
                        };

                        match result {
                            Ok(output) => {
                                if let Some(step) = plan.steps.get_mut(step_index) {
                                    step.set_status(rupoo::task::StepStatus::Completed);
                                    if let rupoo::task::Step::ToolCall { ref mut result, .. } = step {
                                        *result = Some(serde_json::json!({"success": true, "content": &output}));
                                    }
                                    if let rupoo::task::Step::Exec { output: ref mut out, .. } = step {
                                        *out = Some(output);
                                    }
                                }
                                let _ = self.repo.record_step_completion(
                                    &plan.id, step_index,
                                    rupoo::task::StepStatus::Completed,
                                    None,
                                ).await;
                                plan.current_step_index = step_index + 1;
                                plan.updated_at = chrono::Utc::now();
                            }
                            Err(e) => {
                                if let Some(step) = plan.steps.get_mut(step_index) {
                                    step.set_status(rupoo::task::StepStatus::Failed);
                                }
                                let _ = self.repo.record_step_completion(
                                    &plan.id, step_index,
                                    rupoo::task::StepStatus::Failed,
                                    Some(format!("execution error: {e}")),
                                ).await;
                                plan.updated_at = chrono::Utc::now();
                            }
                        }
                        // Continue the plan loop — don't break.
                        continue;
                    }
                    // Normal approval flow: pause and wait for user.
                    self.store_pending_plan(plan, step_index).await;
                    break;
                }
                Err(e) => {
                    let _ = self.ui_tx.send(AgentToTui::Message(
                        ChatMessage::assistant(format!("Error: {e}")),
                    ));
                    let _ = self.ui_tx.send(AgentToTui::Idle);
                    *self.pending_plan.lock().unwrap() = None;
                    *self.pending_step_index.lock().unwrap() = None;
                    break;
                }
            }
        }
    }

    fn send_token_update(&self) {
        if let Some(u) = self.agent.last_usage() {
            let _ = self.ui_tx.send(AgentToTui::TokenUpdate {
                in_count: u.prompt_tokens as u64,
                out_count: u.completion_tokens as u64,
            });
        }
    }

    /// Extract output from all completed steps, not just the first Think.
    pub(super) fn extract_output(&self, plan: &rupoo::task::Plan) -> String {
        let mut outputs = Vec::new();

        for step in &plan.steps {
            let output = match step {
                rupoo::task::Step::Think { output, .. } => output.clone(),
                rupoo::task::Step::Exec { output, .. } => output.clone(),
                rupoo::task::Step::ToolCall { result, .. } => {
                    result.as_ref().map(|r| {
                        serde_json::to_string_pretty(r).unwrap_or_else(|_| r.to_string())
                    })
                }
                _ => None,
            };

            if let Some(o) = output {
                outputs.push(o);
            }
        }

        if outputs.is_empty() {
            return "(no output)".to_string();
        }

        outputs.join("\n\n")
    }

    pub(super) async fn store_pending_plan(
        &self,
        plan: &rupoo::task::Plan,
        step_index: usize,
    ) {
        *self.pending_plan.lock().unwrap() = Some(plan.clone());
        *self.pending_step_index.lock().unwrap() = Some(step_index);
        let (tool_name, params_json) =
            if let Some(rupoo::task::Step::ToolCall {
                ref tool_name,
                ref params,
                ..
            }) = plan.steps.get(step_index)
            {
                (
                    tool_name.clone(),
                    serde_json::to_string_pretty(params).unwrap_or_default(),
                )
            } else {
                ("unknown".into(), "null".into())
            };
        let _ = self.ui_tx.send(AgentToTui::RequestApproval(PendingTool {
            tool_name,
            args: params_json,
        }));
    }
}
