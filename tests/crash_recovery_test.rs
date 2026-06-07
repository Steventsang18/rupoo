use std::sync::Arc;

use rupoo::agent::{Agent, DummyToolExecutor, StepOutcome};
use rupoo::db::TaskRepo;
use rupoo::task::{finish_step, think_step, Plan, StepStatus};

/// Integration test: simulate a process crash after step 0, then resume.
#[tokio::test]
async fn test_crash_recovery_integration() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db_path = tmp.path().to_str().unwrap().to_string();

    // ── First "session": create plan and execute step 0, then "crash" ──
    let plan_id = {
        let repo = Arc::new(TaskRepo::new(&db_path).unwrap());
        let agent = Agent::new(Arc::clone(&repo), Box::new(DummyToolExecutor));

        let steps = vec![
            think_step("first step"),
            think_step("second step"),
            think_step("third step"),
            finish_step("done"),
        ];
        let plan = Plan::new("crash-recovery", steps);
        let pid = plan.id.clone();

        repo.save_plan(&plan).await.unwrap();

        let mut p = agent.resume(&pid).await.unwrap().unwrap();
        let outcome = agent.run_next_step(&mut p).await.unwrap();
        assert!(matches!(outcome, StepOutcome::Advanced));

        pid
        // p is dropped here — simulating "crash" without completing the plan
        // The checkpoint for step 0 is already committed
    };

    // ── Second "session": resume after crash ──
    {
        let repo = Arc::new(TaskRepo::new(&db_path).unwrap());
        let agent = Agent::new(Arc::clone(&repo), Box::new(DummyToolExecutor));

        let mut p = agent.resume(&plan_id).await.unwrap().unwrap();
        // Should resume from step 1 (after step 0's checkpoint)
        assert_eq!(p.current_step_index, 1);

        // Run remaining steps
        let o1 = agent.run_next_step(&mut p).await.unwrap();
        assert!(matches!(o1, StepOutcome::Advanced));
        assert_eq!(p.current_step_index, 2);

        let o2 = agent.run_next_step(&mut p).await.unwrap();
        assert!(matches!(o2, StepOutcome::Advanced));
        assert_eq!(p.current_step_index, 3);

        let o3 = agent.run_next_step(&mut p).await.unwrap();
        assert!(matches!(o3, StepOutcome::Finished));
        assert!(p.is_complete());
    }

    // ── Third session: verify plan is already completed ──
    {
        let repo = Arc::new(TaskRepo::new(&db_path).unwrap());
        let agent = Agent::new(Arc::clone(&repo), Box::new(DummyToolExecutor));

        let result = agent.resume(&plan_id).await.unwrap();
        assert!(result.is_none(), "plan should already be completed");
    }
}

/// Integration test: simulate a crash DURING a step execution (Running checkpoint).
#[tokio::test]
async fn test_interrupted_step_retry() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let db_path = tmp.path().to_str().unwrap().to_string();

    // Create plan with a "Running" checkpoint simulating a crash mid-step
    let steps = vec![
        think_step("step1"),
        think_step("step2"),
        finish_step("done"),
    ];
    let plan = Plan::new("interrupted", steps);
    let plan_id = plan.id.clone();

    {
        let repo = Arc::new(TaskRepo::new(&db_path).unwrap());
        repo.save_plan(&plan).await.unwrap();
    }

    // Manually inject a Running checkpoint for step 0
    {
        let repo = Arc::new(TaskRepo::new(&db_path).unwrap());
        // First complete step 0 normally
        repo.record_step_completion(&plan_id, 0, StepStatus::Completed, None)
            .await
            .unwrap();
        // Then insert a "Running" checkpoint that would represent a crash during step 1
        let running_ckpt = rupoo::task::Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plan_id: plan_id.clone(),
            step_index: 1,
            status: rupoo::task::CheckpointStatus::Running,
            output: None,
            created_at: chrono::Utc::now(),
        };
        repo.save_checkpoint(&running_ckpt).await.unwrap();
    }

    // Resume — should retry step 1 (the interrupted step with Running checkpoint)
    {
        let repo = Arc::new(TaskRepo::new(&db_path).unwrap());
        let agent = Agent::new(Arc::clone(&repo), Box::new(DummyToolExecutor));

        let mut p = agent.resume(&plan_id).await.unwrap().unwrap();
        assert_eq!(p.current_step_index, 1, "should retry the interrupted step");

        let outcome = agent.run_next_step(&mut p).await.unwrap();
        assert!(matches!(outcome, StepOutcome::Advanced));
    }
}
