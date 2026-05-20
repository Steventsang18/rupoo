//! DB layer tests — TaskRepo plan CRUD, checkpoints, settings, UI sessions.
//!
//! Run with: cargo test --test db_test

use std::sync::Arc;
use tempfile::NamedTempFile;
use rupoo::db::TaskRepo;
use rupoo::error::AgentError;
use rupoo::task::{finish_step, think_step, tool_call_step, Checkpoint, CheckpointStatus, Plan, PlanStatus, Step, StepStatus};
use rupoo::shared::{ChatMessage, MessageRole};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_plan(name: &str, steps: Vec<Step>) -> Plan {
    Plan::new(name, steps)
}

fn in_memory_repo() -> Arc<TaskRepo> {
    Arc::new(TaskRepo::new(":memory:").expect("failed to open in-memory db"))
}

fn temp_file_repo() -> Arc<TaskRepo> {
    let tmp = NamedTempFile::new().expect("tempfile failed");
    Arc::new(TaskRepo::new(tmp.path().to_str().unwrap()).expect("failed to open db"))
}

// ---------------------------------------------------------------------------
// Plan lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn save_and_load_plan() -> Result<()> {
    let repo = in_memory_repo();
    let plan = new_plan("test plan", vec![think_step("hello"), finish_step("done")]);

    repo.save_plan(&plan).await?;
    let loaded = repo.load_plan(&plan.id).await?;

    assert_eq!(loaded.id, plan.id);
    assert_eq!(loaded.name, "test plan");
    assert_eq!(loaded.steps.len(), 2);
    assert_eq!(loaded.current_step_index, 0);
    assert_eq!(loaded.status, PlanStatus::Pending);
    Ok(())
}

#[tokio::test]
async fn load_nonexistent_plan_returns_error() -> Result<()> {
    let repo = in_memory_repo();
    let result = repo.load_plan("does-not-exist").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AgentError::PlanNotFound(_)));
    Ok(())
}

// ---------------------------------------------------------------------------
// Step recording
// ---------------------------------------------------------------------------

#[tokio::test]
async fn record_step_completion_advances_step_index() -> Result<()> {
    let repo = in_memory_repo();
    let plan = new_plan("step test", vec![
        think_step("step one"),
        tool_call_step("echo", serde_json::json!({"text":"hi"})),
        finish_step("done"),
    ]);

    repo.save_plan(&plan).await?;
    repo.record_step_completion(&plan.id, 0, StepStatus::Completed, Some("ok".into())).await?;

    let loaded = repo.load_plan(&plan.id).await?;
    assert_eq!(loaded.current_step_index, 1);
    assert_eq!(loaded.status, PlanStatus::Running);
    Ok(())
}

#[tokio::test]
async fn last_step_completion_marks_plan_completed() -> Result<()> {
    let repo = in_memory_repo();
    let plan = new_plan("last step", vec![think_step("only step")]);

    repo.save_plan(&plan).await?;
    repo.record_step_completion(&plan.id, 0, StepStatus::Completed, Some("done".into())).await?;

    let loaded = repo.load_plan(&plan.id).await?;
    assert_eq!(loaded.status, PlanStatus::Completed);
    Ok(())
}

#[tokio::test]
async fn failed_step_marks_plan_failed() -> Result<()> {
    let repo = in_memory_repo();
    let plan = new_plan("fail plan", vec![think_step("will fail")]);

    repo.save_plan(&plan).await?;
    repo.record_step_completion(&plan.id, 0, StepStatus::Failed, Some("__FAILED__".into())).await?;

    let loaded = repo.load_plan(&plan.id).await?;
    assert_eq!(loaded.status, PlanStatus::Failed);
    Ok(())
}

#[tokio::test]
async fn waiting_for_input_preserves_plan_state() -> Result<()> {
    let repo = in_memory_repo();
    let plan = new_plan("wait plan", vec![
        think_step("start"),
        tool_call_step("file_read", serde_json::json!({"path":"/tmp/x"})),
    ]);

    repo.save_plan(&plan).await?;
    repo.record_step_completion(&plan.id, 1, StepStatus::WaitingForInput, None).await?;

    let loaded = repo.load_plan(&plan.id).await?;
    assert_eq!(loaded.status, PlanStatus::WaitingForInput);
    Ok(())
}

#[tokio::test]
async fn record_step_completion_inserts_checkpoint() -> Result<()> {
    let repo = in_memory_repo();
    let plan = new_plan("ckpt test", vec![think_step("a"), finish_step("done")]);

    repo.save_plan(&plan).await?;
    repo.record_step_completion(&plan.id, 0, StepStatus::Completed, Some("output A".into())).await?;

    let ckpt = repo.get_last_checkpoint(&plan.id).await?;
    assert!(ckpt.is_some());
    let ckpt = ckpt.unwrap();
    assert_eq!(ckpt.step_index, 0);
    assert_eq!(ckpt.output.as_deref(), Some("output A"));
    assert_eq!(ckpt.status, CheckpointStatus::Completed);
    Ok(())
}

#[tokio::test]
async fn get_last_checkpoint_returns_none_for_new_plan() -> Result<()> {
    let repo = in_memory_repo();
    let plan = new_plan("no checkpoints", vec![finish_step("done")]);
    repo.save_plan(&plan).await?;

    let ckpt = repo.get_last_checkpoint(&plan.id).await?;
    assert!(ckpt.is_none());
    Ok(())
}

#[tokio::test]
async fn save_checkpoint_standalone_works() -> Result<()> {
    let repo = temp_file_repo();
    let plan = new_plan("standalone ckpt", vec![finish_step("done")]);
    repo.save_plan(&plan).await?;

    let ckpt = Checkpoint {
        id: uuid::Uuid::new_v4().to_string(),
        plan_id: plan.id.clone(),
        step_index: 0,
        status: CheckpointStatus::Completed,
        output: Some("manual output".into()),
        created_at: chrono::Utc::now(),
    };

    repo.save_checkpoint(&ckpt).await?;

    let loaded = repo.get_last_checkpoint(&plan.id).await?.expect("missing checkpoint");
    assert_eq!(loaded.output.as_deref(), Some("manual output"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_setting_upserts_value() -> Result<()> {
    let repo = in_memory_repo();
    repo.set_setting("model", "gpt-4").await?;
    repo.set_setting("model", "claude-3").await?; // overwrite
    // Verify via a second save — if it didn't upsert, second would error
    repo.set_setting("model", "claude-3.5").await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// UI Sessions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn save_and_load_ui_sessions() -> Result<()> {
    let repo = in_memory_repo();

    let msgs = serde_json::to_string(&[
        ChatMessage { role: MessageRole::User, content: "hello".into(), is_command_output: false },
    ])?;

    repo.save_ui_session("sess-1", "Test Session", &msgs, true).await?;

    let sessions = repo.load_ui_sessions().await?;
    assert_eq!(sessions.len(), 1);

    let (id, label, messages_json, is_active) = &sessions[0];
    assert_eq!(id, "sess-1");
    assert_eq!(label, "Test Session");
    assert!(is_active);

    let roundtrip: Vec<ChatMessage> = serde_json::from_str(messages_json)?;
    assert_eq!(roundtrip.len(), 1);
    assert_eq!(roundtrip[0].content, "hello");
    Ok(())
}

#[tokio::test]
async fn load_ui_sessions_empty_when_none() -> Result<()> {
    let repo = in_memory_repo();
    let sessions = repo.load_ui_sessions().await?;
    assert!(sessions.is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Crash recovery
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reset_running_plans_to_pending_recovery() -> Result<()> {
    let repo = temp_file_repo();

    // Create a plan left in Running state (simulating crash mid-execution)
    let plan = new_plan("interrupted", vec![think_step("a"), think_step("b")]);
    repo.save_plan(&plan).await?;

    // Directly set it to Running via record_step_completion
    repo.record_step_completion(&plan.id, 0, StepStatus::Running, None).await?;

    // Now reset
    let recovered = repo.reset_running_plans_to_pending().await?;
    assert!(recovered.contains(&plan.id));

    let loaded = repo.load_plan(&plan.id).await?;
    assert_eq!(loaded.status, PlanStatus::Pending);
    Ok(())
}