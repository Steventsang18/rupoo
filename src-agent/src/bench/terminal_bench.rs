//! Terminal-Bench harness adapter (P1, 报告 §5.2 / §5.3)。
//!
//! Terminal-Bench (<https://github.com/anthropics/terminal-bench>) 用真实终端任务
//! 评测 agent：每个任务目录带一个 `test.sh`（ground-truth 校验脚本）。本模块把 rupoo
//! 包装成可被该 harness 驱动的 agent：rupoo 在任务目录内以非交互 `--run` 模式运行，
//! 之后我们再执行任务的 `test.sh` 判定是否解决。
//!
//! 真正的任务求解需要 LLM + 已构建的 `rupoo` 二进制，因此跑真实任务的集成测试是
//! `#[ignore]` 门控（由 `RUPOO_BENCH=1` + 任务目录触发）。这里的纯逻辑（任务发现、
//! 命令组装）有单测兜底，保证接线正确。

use std::path::Path;
use std::process::Command;

/// 单个 Terminal-Bench 任务：一个含 `test.sh` 的目录。
#[derive(Debug, Clone)]
pub struct BenchTask {
    pub name: String,
    pub dir: std::path::PathBuf,
}

impl BenchTask {
    /// 在 `tasks_root` 下发现所有任务（含 `test.sh` 的子目录）。
    pub fn discover(tasks_root: &Path) -> std::io::Result<Vec<BenchTask>> {
        let mut out = Vec::new();
        if !tasks_root.is_dir() {
            return Ok(out);
        }
        let mut entries: Vec<_> =
            std::fs::read_dir(tasks_root)?.collect::<Result<Vec<_>, std::io::Error>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            if p.is_dir() && p.join("test.sh").is_file() {
                out.push(BenchTask {
                    name: entry.file_name().to_string_lossy().to_string(),
                    dir: p,
                });
            }
        }
        Ok(out)
    }

    /// 组装该任务的 `test.sh` 调用命令。
    pub fn test_command(&self) -> Command {
        let mut cmd = Command::new("bash");
        cmd.arg(self.dir.join("test.sh")).current_dir(&self.dir);
        cmd
    }
}

/// 运行 rupoo 并对任务做评估的结果。
#[derive(Debug)]
pub struct BenchResult {
    pub task: String,
    pub solved: bool,
    pub test_output: String,
}

/// 在任务目录内以非交互 `--run` 模式驱动 rupoo，再执行 `test.sh` 判定。
///
/// * `rupoo_bin` —— 已构建的 rupoo 二进制路径（如 `./target/release/rupoo`
///   或安装的 `~/.cargo/bin/rupoo`）。
/// * `task_prompt` —— 传给 rupoo 的指令（通常为任务的 `description` / `prompt`）。
pub fn run_task(
    rupoo_bin: &Path,
    task: &BenchTask,
    task_prompt: &str,
) -> std::io::Result<BenchResult> {
    // 在任务目录内以 --run（非交互）模式运行 rupoo。
    let _agent = Command::new(rupoo_bin)
        .arg("--run")
        .arg(task_prompt)
        .current_dir(&task.dir)
        .output()?;

    // 评估 ground-truth 测试。
    let test = task.test_command().output()?;
    let test_output = String::from_utf8_lossy(&test.stdout).to_string();
    Ok(BenchResult {
        task: task.name.clone(),
        solved: test.status.success(),
        test_output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_finds_only_dirs_with_test_sh() {
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
    fn test_command_targets_test_sh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("t1")).unwrap();
        std::fs::write(dir.path().join("t1/test.sh"), "echo ok\n").unwrap();
        let task = BenchTask::discover(dir.path()).unwrap().remove(0);
        let cmd = task.test_command();
        assert_eq!(cmd.get_program(), "bash");
    }
}
