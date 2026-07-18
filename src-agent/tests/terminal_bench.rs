//! Terminal-Bench 集成（P1）。
//!
//! 真实任务求解需要 LLM + 已构建的 rupoo 二进制，因此跑真实任务的用例是
//! `#[ignore]` 门控（由 `RUPOO_BENCH=1` 触发，配合任务目录）。纯逻辑
//! （任务发现 / 命令组装）有单测兜底，保证接线正确、可编译。

use rupoo::bench::terminal_bench::{BenchResult, BenchTask};
use std::path::Path;

#[test]
fn bench_task_discovery_finds_test_sh_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("task_a")).unwrap();
    std::fs::write(root.join("task_a/test.sh"), "exit 0\n").unwrap();
    std::fs::create_dir_all(root.join("task_b")).unwrap(); // 无 test.sh
    std::fs::write(root.join("task_b/notes.txt"), "x\n").unwrap();

    let tasks = BenchTask::discover(root).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "task_a");
}

#[test]
fn bench_task_test_command_targets_test_sh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("t1")).unwrap();
    std::fs::write(dir.path().join("t1/test.sh"), "echo ok\n").unwrap();
    let task = BenchTask::discover(dir.path()).unwrap().remove(0);
    let cmd = task.test_command();
    assert_eq!(cmd.get_program(), "bash");
}

/// 真实跑分：需 `RUPOO_BENCH=1` + 已构建 rupoo 二进制 + 任务目录。
/// 由 `.github/workflows/terminal-bench.yml` 每周 cron 触发，本地可用
/// `RUPOO_BENCH=1 cargo test -p rupoo --test terminal_bench -- --ignored` 运行。
#[test]
#[ignore]
fn terminal_bench_real_run() {
    let tasks_root =
        std::env::var("RUPOO_BENCH_TASKS").unwrap_or_else(|_| "./terminal-bench/tasks".to_string());
    let rupoo_bin =
        std::env::var("RUPOO_BENCH_BIN").unwrap_or_else(|_| "./target/release/rupoo".to_string());
    if !Path::new(&rupoo_bin).exists() {
        eprintln!("skip: rupoo binary not found at {rupoo_bin}");
        return;
    }
    let tasks = BenchTask::discover(Path::new(&tasks_root)).unwrap_or_default();
    let mut solved = 0usize;
    for task in &tasks {
        let prompt = std::fs::read_to_string(task.dir.join("prompt")).unwrap_or_default();
        if let Ok(BenchResult { solved: true, .. }) =
            rupoo::bench::terminal_bench::run_task(Path::new(&rupoo_bin), task, &prompt)
        {
            solved += 1;
        }
    }
    println!("Terminal-Bench: {solved}/{} tasks solved", tasks.len());
}
