//! Plan execution — extracted from main.rs.

use std::sync::Arc;
use tracing::info;

use rupoo::agent::Agent;
use rupoo::db::TaskRepo;

pub async fn execute_plan(
    _repo: &Arc<TaskRepo>,
    agent: &Agent,
    plan_id: &str,
    input: Option<&str>,
) -> anyhow::Result<()> {
    info!(plan_id = %plan_id, "starting/resuming plan");

    let mut plan = match agent.resume(plan_id).await? {
        Some(p) => p,
        None => {
            info!("plan already completed, nothing to do");
            return Ok(());
        }
    };

    info!(
        plan_id = %plan_id,
        status = ?plan.status,
        current_step = plan.current_step_index,
        total_steps = plan.steps.len(),
        "plan loaded"
    );

    loop {
        let step_index = plan.current_step_index;

        if let Some(input_val) = input {
            let is_waiting = plan.steps.get(step_index).is_some_and(|s| s.is_waiting());
            if is_waiting {
                info!(plan_id = %plan_id, step = step_index, input = %input_val, "injecting input into WaitForInput step");
                let outcome = agent.inject_input(&mut plan, step_index, input_val).await?;
                match outcome {
                    rupoo::agent::StepOutcome::Advanced => {
                        info!(plan_id = %plan_id, step = step_index, "input injected, step completed");
                        continue;
                    }
                    _ => {
                        anyhow::bail!("unexpected outcome from inject_input: {:?}", outcome);
                    }
                }
            }
        }

        let outcome = agent.run_next_step(&mut plan).await?;

        match outcome {
            rupoo::agent::StepOutcome::Advanced => {
                info!(
                    plan_id = %plan_id,
                    step = step_index,
                    "step completed"
                );
            }
            rupoo::agent::StepOutcome::Finished => {
                info!(plan_id = %plan_id, "plan execution finished");
                print_plan_result(&plan);
                break;
            }
            rupoo::agent::StepOutcome::WaitingForInput(prompt) => {
                println!("\n=== WAITING FOR INPUT ===");
                println!("Prompt: {prompt}");
                println!("==========================");
                if input.is_some() {
                    println!("Input was provided but the step type was not WaitForInput at resume time.");
                } else {
                    println!("The plan is paused. Re-run with `--input <text>` to provide input.");
                }
                break;
            }
            rupoo::agent::StepOutcome::Failed(err) => {
                println!("\n=== PLAN FAILED ===");
                println!("Error: {err}");
                println!("====================\n");
                break;
            }
            rupoo::agent::StepOutcome::RequiresApproval { .. } => {
                println!("\n=== REQUIRES APPROVAL (TUI only) ===");
                println!("This plan has a tool that requires TUI approval.");
                println!("Run it with the interactive TUI (rupoo without --run flag) instead.");
                break;
            }
        }
    }

    Ok(())
}

fn print_plan_result(plan: &rupoo::task::Plan) {
    println!("\n=== PLAN RESULT ===");
    println!("Name: {}", plan.name);
    println!("Status: {:?}", plan.status);
    for (i, step) in plan.steps.iter().enumerate() {
        let status_mark = match step.status() {
            rupoo::task::StepStatus::Completed => "✓",
            rupoo::task::StepStatus::Failed => "✗",
            rupoo::task::StepStatus::Running => "▶",
            rupoo::task::StepStatus::Pending => "·",
            rupoo::task::StepStatus::WaitingForInput => "⊘",
        };

        let label = match step {
            rupoo::task::Step::Think { instruction, output, .. } => {
                format!("THINK: {instruction} | out: {}", output.as_deref().unwrap_or("-"))
            }
            rupoo::task::Step::ToolCall { tool_name, result, .. } => {
                let r = result
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string());
                format!("TOOL: {tool_name} | result: {r}")
            }
            rupoo::task::Step::WaitForInput { prompt, response, .. } => {
                format!("WAIT: {prompt} | response: {}", response.as_deref().unwrap_or("(pending)"))
            }
            rupoo::task::Step::Finish { summary, .. } => format!("FINISH: {summary}"),
            rupoo::task::Step::Exec { command, output, .. } => {
                format!("EXEC: {command} | out: {}", output.as_deref().unwrap_or("-"))
            }
            rupoo::task::Step::HttpRequest { url, response, .. } => {
                format!("HTTP: {url} | resp: {}", response.as_deref().unwrap_or("-"))
            }
            rupoo::task::Step::BrowserAction { action, output, .. } => {
                format!("BROWSER: {action:?} | out: {}", output.as_deref().unwrap_or("-"))
            }
        };
        println!("  {status_mark} [{i}] {label}");
    }
    println!("========================\n");
}
