use rupoo::db::TaskRepo;
use rupoo::task::Plan;

#[tokio::test]
async fn test_list_plains_empty() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let repo = TaskRepo::new(db_path.to_str().unwrap()).unwrap();
    let plans = repo.list_plans(10, 0).await.unwrap();
    assert!(plans.is_empty());
    let counts = repo.count_plans_by_status().await.unwrap();
    assert!(counts.is_empty());
}

#[tokio::test]
async fn test_crud_plan() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let repo = TaskRepo::new(db_path.to_str().unwrap()).unwrap();
    let plan = Plan::new("Test", vec![]);
    repo.save_plan(&plan).await.unwrap();
    let plans = repo.list_plans(10, 0).await.unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].name, "Test");
    repo.delete_plan(&plan.id).await.unwrap();
    let plans = repo.list_plans(10, 0).await.unwrap();
    assert!(plans.is_empty());
}

#[tokio::test]
async fn test_count_memories() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let repo = TaskRepo::new(db_path.to_str().unwrap()).unwrap();

    // Initially zero
    assert_eq!(repo.count_memories().await.unwrap(), 0);

    // Store a memory
    repo.store_memory("test content", &["test"], "cli_test").await.unwrap();
    assert_eq!(repo.count_memories().await.unwrap(), 1);
}

#[tokio::test]
async fn test_prune_plans() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let repo = TaskRepo::new(db_path.to_str().unwrap()).unwrap();

    let plan = Plan::new("Old Plan", vec![]);
    repo.save_plan(&plan).await.unwrap();

    // Mark as completed by recording step completion
    // prunes plans older than 1 second from now — our plan was just created
    tokio::time::sleep(tokio::time::Duration::from_millis(1100)).await;

    // Complete the plan via direct DB to set status
    // Alternative: use the plan's status field in a new plan that's pre-completed
    let before = chrono::Utc::now().to_rfc3339();
    let deleted = repo.prune_plans(&before).await.unwrap();
    assert_eq!(deleted, 0); // not completed yet

    // Create a completed plan by saving and then completing
    let plan2 = Plan::new("Old Completed", vec![
        rupoo::task::finish_step("done"),
    ]);
    repo.save_plan(&plan2).await.unwrap();
    repo.record_step_completion(&plan2.id, 0, rupoo::task::StepStatus::Completed, None).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let before2 = chrono::Utc::now().to_rfc3339();
    let deleted2 = repo.prune_plans(&before2).await.unwrap();
    assert_eq!(deleted2, 1);
}
