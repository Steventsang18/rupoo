//! Integration tests for core Rupoo workflows.
//!
//! Tests the full flow of Plan creation → execution → completion,
//! approval workflows, memory operations, conversation persistence,
//! and crash recovery.

use std::sync::Arc;
use tempfile::TempDir;

use rupoo::agent::{Agent, DummyToolExecutor};
use rupoo::db::TaskRepo;
use rupoo::llm::ConversationHistory;
use rupoo::skill::{SkillDef, SkillManager, SkillStep};
use rupoo::task::{
    browser_action_step, exec_step, finish_step, http_request_step, think_step,
    tool_call_step, wait_for_input_step, BrowserActionType, CheckpointStatus, HttpMethod, Plan, PlanStatus,
    StepStatus,
};
use rupoo::agent::StepOutcome;

/// Helper to create a test repo with temp directory.
fn test_repo() -> (TempDir, Arc<TaskRepo>) {
    let tmp = TempDir::new().unwrap();
    let repo = Arc::new(TaskRepo::new(tmp.path().join("test.db").to_str().unwrap()).unwrap());
    (tmp, repo)
}

/// Helper to create a test agent.
fn test_agent(repo: Arc<TaskRepo>) -> Agent {
    Agent::new(repo, Box::new(DummyToolExecutor))
}

// ---------------------------------------------------------------------------
// Test: Plan creation → execution → completion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_plan_creation_and_execution() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    // Create a plan with Think + ToolCall (echo) + Finish steps
    let steps = vec![
        think_step("analyze the task"),
        tool_call_step("echo", serde_json::json!({"message": "hello"})),
        finish_step("task complete"),
    ];
    let plan = Plan::new("test-plan", steps);
    let plan_id = plan.id.clone();

    // Save the plan
    repo.save_plan(&plan).await.unwrap();

    // Resume and execute
    let mut resumed_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    assert_eq!(resumed_plan.status, PlanStatus::Running);
    assert_eq!(resumed_plan.current_step_index, 0);

    // Execute think step
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Advanced));
    assert_eq!(resumed_plan.current_step_index, 1);

    // Execute tool call step
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Advanced));
    assert_eq!(resumed_plan.current_step_index, 2);

    // Execute finish step
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Finished));
    assert!(resumed_plan.is_complete());
}

// ---------------------------------------------------------------------------
// Test: Approval workflow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_approval_workflow() {
    let (_tmp, repo) = test_repo();
    
    // Create agent with default safety
    let agent = test_agent(repo.clone());

    // Create a plan with a tool call step
    let steps = vec![
        think_step("prepare"),
        tool_call_step("echo", serde_json::json!({"message": "approval test"})),
        finish_step("done"),
    ];
    let plan = Plan::new("approval-test", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();

    // Resume and execute - the echo tool should not require approval
    let mut resumed_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    
    // Execute think
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Advanced));
    
    // Execute tool call - should succeed without approval
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Advanced));
    
    // Execute finish
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Finished));
}

// ---------------------------------------------------------------------------
// Test: Memory store and retrieve
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_memory_store_and_retrieve() {
    let (_tmp, repo) = test_repo();

    // Store a memory
    let mem_id = repo
        .store_memory(
            "Rust is a systems programming language",
            &["rust", "programming"],
            "test",
        )
        .await
        .unwrap();

    assert!(!mem_id.is_empty());

    // Search for it
    let results = repo.search_memories("Rust programming", 5).await.unwrap();
    
    assert!(!results.is_empty());
    assert!(results.iter().any(|r| r.content.contains("Rust")));
    
    // Search with different query
    let results2 = repo.search_memories("systems language", 5).await.unwrap();
    assert!(!results2.is_empty());
    
    // Get recent memories
    let recent = repo.recent_memories(10).await.unwrap();
    assert!(!recent.is_empty());
    assert_eq!(recent[0].id, mem_id);
}

// ---------------------------------------------------------------------------
// Test: Conversation history persistence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_conversation_history_persistence() {
    let (_tmp, repo) = test_repo();

    // Create and save a conversation history
    let mut history = ConversationHistory::new(10);
    history.push_user("Hello");
    history.push_assistant("Hi there!");
    history.push_user("How are you?");
    history.push_assistant("I'm doing great, thanks for asking!");

    repo.save_conversation_history("session-123", &history)
        .await
        .unwrap();

    // Load it back
    let loaded = repo
        .load_conversation_history("session-123")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(loaded.message_count(), 4);
    
    // Verify message count matches (can't access individual messages directly)
    // We can verify by creating a new history and comparing
    let mut history = ConversationHistory::new(10);
    history.push_user("Hello");
    history.push_assistant("Hi there!");
    history.push_user("How are you?");
    history.push_assistant("I'm doing great, thanks for asking!");
    
    // Both should have the same message count
    assert_eq!(loaded.message_count(), history.message_count());
}

// ---------------------------------------------------------------------------
// Test: Non-existent session returns None
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_nonexistent_session_returns_none() {
    let (_tmp, repo) = test_repo();

    let result = repo
        .load_conversation_history("nonexistent-session")
        .await
        .unwrap();

    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// Test: Crash recovery from Running step
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_crash_recovery_from_running_step() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    // Create a plan
    let steps = vec![
        think_step("phase1"),
        think_step("phase2"),
        finish_step("recovered"),
    ];
    let plan = Plan::new("crash-test", steps);
    let plan_id = plan.id.clone();
    repo.save_plan(&plan).await.unwrap();

    // Simulate: step 0 was running when crash occurred
    repo.record_step_completion(&plan_id, 0, StepStatus::Running, None)
        .await
        .unwrap();

    // Recovery should detect the Running checkpoint and resume from step 0
    let recovered_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    assert_eq!(recovered_plan.current_step_index, 0);
    assert_eq!(recovered_plan.status, PlanStatus::Running);
}

// ---------------------------------------------------------------------------
// Test: Crash recovery from Completed step
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_crash_recovery_from_completed_step() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    // Create a plan with 3 steps
    let steps = vec![
        think_step("phase1"),
        think_step("phase2"),
        think_step("phase3"),
        finish_step("done"),
    ];
    let plan = Plan::new("resume-test", steps);
    let plan_id = plan.id.clone();
    repo.save_plan(&plan).await.unwrap();

    // Simulate: steps 0 and 1 completed, then crash
    repo.record_step_completion(&plan_id, 0, StepStatus::Completed, Some("step 0 done".into()))
        .await
        .unwrap();
    repo.record_step_completion(&plan_id, 1, StepStatus::Completed, Some("step 1 done".into()))
        .await
        .unwrap();

    // Recovery should resume from step 2
    let recovered_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    assert_eq!(recovered_plan.current_step_index, 2);
    assert_eq!(recovered_plan.status, PlanStatus::Running);
}

// ---------------------------------------------------------------------------
// Test: Exec step execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exec_step_execution() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    // Create a plan with an Exec step
    let steps = vec![
        exec_step("echo", vec!["hello".to_string()], Some(5)),
        finish_step("exec complete"),
    ];
    let plan = Plan::new("exec-test", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();

    let mut resumed_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    
    // Execute exec step
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    
    // Should succeed or fail depending on tool availability
    match outcome {
        StepOutcome::Advanced => {
            assert_eq!(resumed_plan.current_step_index, 1);
        }
        StepOutcome::Failed(err) => {
            // Some systems may not have echo, that's OK
            assert!(err.contains("error") || err.contains("not found") || err.contains("failed"));
        }
        _ => panic!("unexpected outcome: {:?}", outcome),
    }
}

// ---------------------------------------------------------------------------
// Test: HttpRequest step execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_http_request_step() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    // Create a plan with HTTP request step (GET httpbin)
    let steps = vec![
        http_request_step(
            "https://httpbin.org/get",
            HttpMethod::GET,
            None,
            None,
        ),
        finish_step("http complete"),
    ];
    let plan = Plan::new("http-test", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();

    let mut resumed_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    
    // Execute HTTP request
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    
    // Should advance (or fail if network unavailable)
    match outcome {
        StepOutcome::Advanced => {
            // Check the step result
            if let Some(rupoo::task::Step::HttpRequest { response, .. }) = resumed_plan.steps.get(0) {
                assert!(response.is_some());
            }
        }
        StepOutcome::Failed(_) => {
            // Network may be unavailable in test environment
        }
        _ => panic!("unexpected outcome: {:?}", outcome),
    }
}

// ---------------------------------------------------------------------------
// Test: Browser action step
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_browser_action_step() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    // Create a plan with Navigate browser action
    let steps = vec![
        browser_action_step(
            BrowserActionType::Navigate,
            Some("https://example.com".to_string()),
            None,
            Some(10),
        ),
        finish_step("browser complete"),
    ];
    let plan = Plan::new("browser-test", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();

    let mut resumed_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    
    // Execute browser action
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    
    // Browser may not be available, so handle both cases
    match outcome {
        StepOutcome::Advanced => {
            if let Some(rupoo::task::Step::BrowserAction { output, .. }) = resumed_plan.steps.get(0) {
                assert!(output.is_some());
            }
        }
        StepOutcome::Failed(err) => {
            // Browser may not be installed
            assert!(err.contains("not found") || err.contains("No supported browser"));
        }
        _ => panic!("unexpected outcome: {:?}", outcome),
    }
}

// ---------------------------------------------------------------------------
// Test: WaitForInput does not advance index
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wait_for_input_preserves_index() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    let steps = vec![
        think_step("prepare"),
        wait_for_input_step("Enter your name:"),
        finish_step("input received"),
    ];
    let plan = Plan::new("wait-test", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();

    let mut resumed_plan = agent.resume(&plan_id).await.unwrap().unwrap();
    
    // Execute think
    agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert_eq!(resumed_plan.current_step_index, 1);
    
    // Execute wait_for_input - should NOT advance
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    
    assert!(matches!(outcome, StepOutcome::WaitingForInput(_)));
    assert_eq!(resumed_plan.current_step_index, 1); // Index unchanged!
    assert_eq!(resumed_plan.status, PlanStatus::WaitingForInput);
    
    // Now inject input
    let outcome = agent.inject_input(&mut resumed_plan, 1, "Alice").await.unwrap();
    assert!(matches!(outcome, StepOutcome::Advanced));
    assert_eq!(resumed_plan.current_step_index, 2);
    
    // Finish
    let outcome = agent.run_next_step(&mut resumed_plan).await.unwrap();
    assert!(matches!(outcome, StepOutcome::Finished));
}

// ---------------------------------------------------------------------------
// Test: Skill creation and execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_skill_to_plan_conversion() {
    let tmp = TempDir::new().unwrap();
    let manager = SkillManager::new(tmp.path().join(".skills"));
    manager.ensure_dir().unwrap();

    // Create a skill with various step types
    let skill = SkillDef {
        name: "test-skill".to_string(),
        description: "A test skill".to_string(),
        version: "1.0".to_string(),
        schema_version: "2.0".to_string(),
        trigger: vec!["test".to_string()],
        steps: vec![
            SkillStep::Think {
                instruction: "analyze".to_string(),
            },
            SkillStep::Exec {
                command: "ls".to_string(),
                args: vec!["-la".to_string()],
                timeout_secs: Some(10),
            },
            SkillStep::HttpRequest {
                url: "https://example.com".to_string(),
                method: "GET".to_string(),
                body: None,
                headers: None,
            },
            SkillStep::Finish {
                summary: "done".to_string(),
            },
        ],
    };

    manager.save_skill(&skill).unwrap();

    // Load and convert to plan
    let loaded = manager.load_skill("test-skill").unwrap();
    let plan = manager.skill_to_plan(&loaded);

    assert_eq!(plan.name, "test-skill");
    assert_eq!(plan.steps.len(), 4);
    assert!(matches!(plan.steps[0], rupoo::task::Step::Think { .. }));
    assert!(matches!(plan.steps[1], rupoo::task::Step::Exec { .. }));
    assert!(matches!(plan.steps[2], rupoo::task::Step::HttpRequest { .. }));
    assert!(matches!(plan.steps[3], rupoo::task::Step::Finish { .. }));
}

// ---------------------------------------------------------------------------
// Test: Plan to Skill conversion preserves types
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_plan_to_skill_preserves_types() {
    let steps = vec![
        think_step("analyze"),
        exec_step("echo", vec!["test".to_string()], None),
        http_request_step("https://api.example.com", HttpMethod::POST, Some("{}".into()), None),
        browser_action_step(BrowserActionType::GetText, Some("https://example.com".into()), None, Some(30)),
        finish_step("complete"),
    ];
    
    let mut plan = Plan::new("complex-plan", steps);
    
    // Mark all steps as completed
    for step in &mut plan.steps {
        step.set_status(StepStatus::Completed);
    }

    let skill = SkillManager::plan_to_skill(&plan, "complex-skill", "A complex skill");
    
    assert_eq!(skill.schema_version, "2.0");
    assert_eq!(skill.steps.len(), 5);
    
    assert!(matches!(skill.steps[0], SkillStep::Think { .. }));
    assert!(matches!(skill.steps[1], SkillStep::Exec { .. }));
    assert!(matches!(skill.steps[2], SkillStep::HttpRequest { .. }));
    assert!(matches!(skill.steps[3], SkillStep::BrowserAction { .. }));
    assert!(matches!(skill.steps[4], SkillStep::Finish { .. }));
}

// ---------------------------------------------------------------------------
// Test: Settings CRUD operations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_settings_crud() {
    let (_tmp, repo) = test_repo();

    // Set settings
    repo.set_setting("api_key", "secret123").await.unwrap();
    repo.set_setting("theme", "dark").await.unwrap();
    repo.set_setting("timeout", "30").await.unwrap();

    // Get individual setting
    let api_key = repo.get_setting("api_key").await.unwrap().unwrap();
    assert_eq!(api_key, "secret123");

    // List all settings
    let settings = repo.list_settings().await.unwrap();
    assert_eq!(settings.len(), 3);
    assert!(settings.iter().any(|(k, _)| k == "api_key"));
    assert!(settings.iter().any(|(k, _)| k == "theme"));

    // Delete a setting
    repo.delete_setting("theme").await.unwrap();
    
    let settings_after = repo.list_settings().await.unwrap();
    assert_eq!(settings_after.len(), 2);
    assert!(!settings_after.iter().any(|(k, _)| k == "theme"));
}

// ---------------------------------------------------------------------------
// Test: Heartbeat checkpoint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_heartbeat_creates_running_checkpoint() {
    let (_tmp, repo) = test_repo();
    let agent = test_agent(repo.clone());

    let steps = vec![think_step("long task"), finish_step("done")];
    let plan = Plan::new("heartbeat-test", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();

    // Emit heartbeat
    agent.heartbeat(&plan_id, 0).await.unwrap();

    // Check checkpoint
    let ckpt = repo.get_last_checkpoint(&plan_id).await.unwrap().unwrap();
    assert_eq!(ckpt.step_index, 0);
    assert_eq!(ckpt.status, CheckpointStatus::Running);
}

// ---------------------------------------------------------------------------
// Test: Plan deletion cascades to checkpoints
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_plan_deletion_cascades() {
    let (_tmp, repo) = test_repo();

    let steps = vec![think_step("temp"), finish_step("done")];
    let plan = Plan::new("temp-plan", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();
    
    // Add some checkpoints
    repo.record_step_completion(&plan_id, 0, StepStatus::Completed, Some("done".into()))
        .await
        .unwrap();

    // Verify checkpoint exists
    let ckpt = repo.get_last_checkpoint(&plan_id).await.unwrap();
    assert!(ckpt.is_some());

    // Delete the plan
    repo.delete_plan(&plan_id).await.unwrap();

    // Verify plan is gone
    let loaded = repo.load_plan(&plan_id).await;
    assert!(loaded.is_err());

    // Verify checkpoints are also gone
    let ckpt_after = repo.get_last_checkpoint(&plan_id).await.unwrap();
    assert!(ckpt_after.is_none());
}

// ---------------------------------------------------------------------------
// Test: List and count plans
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_and_count_plans() {
    let (_tmp, repo) = test_repo();

    // Create multiple plans
    for i in 0..5 {
        let steps = vec![think_step(&format!("task {}", i))];
        let plan = Plan::new(&format!("plan-{}", i), steps);
        repo.save_plan(&plan).await.unwrap();
    }

    // List plans
    let summaries = repo.list_plans(10, 0).await.unwrap();
    assert_eq!(summaries.len(), 5);

    // Count by status
    let counts = repo.count_plans_by_status().await.unwrap();
    let pending_count = counts.iter().find(|(s, _)| s == "Pending").map(|(_, c)| *c);
    assert_eq!(pending_count, Some(5));
}

// ---------------------------------------------------------------------------
// Test: Plan summary fields
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_plan_summary_fields() {
    let (_tmp, repo) = test_repo();

    let steps = vec![
        think_step("step1"),
        think_step("step2"),
        think_step("step3"),
        finish_step("done"),
    ];
    let plan = Plan::new("summary-test", steps);
    let plan_id = plan.id.clone();

    repo.save_plan(&plan).await.unwrap();

    let summaries = repo.list_plans(10, 0).await.unwrap();
    let summary = summaries.first().unwrap();

    assert_eq!(summary.name, "summary-test");
    assert_eq!(summary.total_steps, 4);
    assert_eq!(summary.status, "Pending");
}
