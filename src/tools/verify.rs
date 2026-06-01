#![allow(refining_impl_trait)]

//! Verification tool belt for Rupoo.
//!
//! Tools that let the LLM **verify** its own work — run tests, check output,
//! diff changes. These are the "reins" that constrain quality: the LLM
//! explores freely but can self-check before delivering.
//!
//! Design principle: verification tools don't constrain *how* the LLM works,
//! they constrain *whether the output is correct*. This is the sustainable
//! kind of external constraint — it scales with model capability.

use serde::{Deserialize, Serialize};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use rig::wasm_compat::{WasmCompatSend, WasmCompatSync};

use crate::signal;

// ---------------------------------------------------------------------------
// Run Tests — detect project type and run its test suite
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RunTestsArgs {
    /// Optional path to the project directory (defaults to CWD)
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct RunTestsOutput {
    pub success: bool,
    pub output: String,
    pub test_runner: String,
    pub error: Option<String>,
}

pub struct RunTestsTool;

#[allow(clippy::manual_async_fn)]
impl Tool for RunTestsTool {
    const NAME: &'static str = "run_tests";
    type Error = crate::rig_tools::ToolCallError;
    type Args = RunTestsArgs;
    type Output = RunTestsOutput;

    fn name(&self) -> String {
        "run_tests".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = ToolDefinition> + WasmCompatSend + WasmCompatSync {
        async move {
            ToolDefinition {
                name: "run_tests".into(),
                description: "Run the project's test suite. Auto-detects Rust (cargo test), Node.js (npm test), Python (pytest), and Go (go test).".into(),
                parameters: crate::tools::schema::run_tests(),
            }
        }
    }

    fn call(
        &self,
        args: RunTestsArgs,
    ) -> impl std::future::Future<Output = Result<RunTestsOutput, Self::Error>> + WasmCompatSend + WasmCompatSync {
        async move {
            let dir = args.path.unwrap_or_else(|| ".".to_string());
            let dir_path = std::path::Path::new(&dir);

            // Detect project type and run appropriate test command
            let (cmd, runner) = if dir_path.join("Cargo.toml").exists() {
                (vec!["cargo", "test", "--lib"], "cargo test")
            } else if dir_path.join("package.json").exists() {
                (vec!["npm", "test"], "npm test")
            } else if dir_path.join("go.mod").exists() {
                (vec!["go", "test", "./..."], "go test")
            } else if dir_path.join("pytest.ini").exists()
                || dir_path.join("pyproject.toml").exists()
                || std::fs::read_dir(dir_path)
                    .ok()
                    .map(|d| d.take(100).any(|e| e.ok().map(|e| e.file_name().to_string_lossy().ends_with("_test.py")).unwrap_or(false)))
                    .unwrap_or(false)
            {
                (vec!["pytest"], "pytest")
            } else {
                return Ok(RunTestsOutput {
                    success: false,
                    output: String::new(),
                    test_runner: "unknown".into(),
                    error: Some("No recognized project type found (expected Cargo.toml, package.json, go.mod, or pytest config)".into()),
                });
            };

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                tokio::process::Command::new(cmd[0])
                    .args(&cmd[1..])
                    .current_dir(&dir)
                    .output(),
            ).await;

            match result {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let combined = if stderr.is_empty() {
                        stdout.to_string()
                    } else {
                        format!("{stdout}\n{stderr}")
                    };

                    let compressed = signal::compress_output(&combined, Some(6000));
                    let success = output.status.success();

                    Ok(RunTestsOutput {
                        success,
                        output: compressed,
                        test_runner: runner.into(),
                        error: if success { None } else { Some(format!("Test runner exited with code {}", output.status.code().unwrap_or(-1))) },
                    })
                }
                Err(_) => {
                    // Timeout
                    Ok(RunTestsOutput {
                        success: false,
                        output: String::new(),
                        test_runner: runner.into(),
                        error: Some("Test runner timed out after 120 seconds".into()),
                    })
                }
                Ok(Err(e)) => {
                    Ok(RunTestsOutput {
                        success: false,
                        output: String::new(),
                        test_runner: runner.into(),
                        error: Some(format!("Failed to run test command: {e}")),
                    })
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Check Output — run a program and capture its output
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CheckOutputArgs {
    /// Command to run
    pub command: String,
    /// Command-line arguments (space-separated)
    pub args: Option<String>,
    /// Working directory (defaults to CWD)
    pub cwd: Option<String>,
    /// Timeout in seconds (default 30)
    pub timeout: Option<u64>,
}

#[derive(Serialize)]
pub struct CheckOutputOutput {
    pub success: bool,
    pub stdout: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

pub struct CheckOutputTool;

#[allow(clippy::manual_async_fn)]
impl Tool for CheckOutputTool {
    const NAME: &'static str = "check_output";
    type Error = crate::rig_tools::ToolCallError;
    type Args = CheckOutputArgs;
    type Output = CheckOutputOutput;

    fn name(&self) -> String {
        "check_output".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = ToolDefinition> + WasmCompatSend + WasmCompatSync {
        async move {
            ToolDefinition {
                name: "check_output".into(),
                description: "Run a command and capture its output. Use this to verify your code works — run the program and check the result. Safer than raw shell: has timeout and output size limits.".into(),
                parameters: crate::tools::schema::check_output(),
            }
        }
    }

    fn call(
        &self,
        args: CheckOutputArgs,
    ) -> impl std::future::Future<Output = Result<CheckOutputOutput, Self::Error>> + WasmCompatSend + WasmCompatSync {
        async move {
            let timeout_secs = args.timeout.unwrap_or(30).min(120);
            let safety = crate::safety::SafetyContext::default();

            // Safety check: block dangerous commands
            if let Err(e) = safety.validate_command(&args.command) {
                return Ok(CheckOutputOutput {
                    success: false,
                    stdout: String::new(),
                    exit_code: None,
                    error: Some(e.to_string()),
                });
            }

            // Parse args
            let arg_list: Vec<&str> = args.args
                .as_deref()
                .map(|a| a.split_whitespace().collect())
                .unwrap_or_default();

            let mut cmd = tokio::process::Command::new(&args.command);
            cmd.args(&arg_list)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            if let Some(ref cwd) = args.cwd {
                cmd.current_dir(cwd);
            }

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                cmd.output(),
            ).await;

            match result {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let combined = if stderr.is_empty() {
                        stdout.to_string()
                    } else {
                        format!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}")
                    };

                    let compressed = signal::compress_output(&combined, Some(6000));
                    let success = output.status.success();

                    Ok(CheckOutputOutput {
                        success,
                        stdout: compressed,
                        exit_code: output.status.code(),
                        error: if success { None } else { Some(format!("Command exited with code {}", output.status.code().unwrap_or(-1))) },
                    })
                }
                Err(_) => {
                    // Timeout
                    Ok(CheckOutputOutput {
                        success: false,
                        stdout: String::new(),
                        exit_code: None,
                        error: Some(format!("Command timed out after {} seconds", timeout_secs)),
                    })
                }
                Ok(Err(e)) => {
                    Ok(CheckOutputOutput {
                        success: false,
                        stdout: String::new(),
                        exit_code: None,
                        error: Some(format!("Failed to run command: {e}")),
                    })
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Diff Check — compare git changes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DiffCheckArgs {
    /// What to diff: "staged", "unstaged", or "all" (default: "all")
    pub scope: Option<String>,
    /// Path to the project directory (defaults to CWD)
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct DiffCheckOutput {
    pub success: bool,
    pub diff: String,
    pub stats: String,
    pub error: Option<String>,
}

pub struct DiffCheckTool;

#[allow(clippy::manual_async_fn)]
impl Tool for DiffCheckTool {
    const NAME: &'static str = "diff_check";
    type Error = crate::rig_tools::ToolCallError;
    type Args = DiffCheckArgs;
    type Output = DiffCheckOutput;

    fn name(&self) -> String {
        "diff_check".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = ToolDefinition> + WasmCompatSend + WasmCompatSync {
        async move {
            ToolDefinition {
                name: "diff_check".into(),
                description: "Check git diff to review your changes before committing. Shows what was added, removed, or modified. Use this to verify your code changes are correct.".into(),
                parameters: crate::tools::schema::diff_check(),
            }
        }
    }

    fn call(
        &self,
        args: DiffCheckArgs,
    ) -> impl std::future::Future<Output = Result<DiffCheckOutput, Self::Error>> + WasmCompatSend + WasmCompatSync {
        async move {
            let dir = args.path.unwrap_or_else(|| ".".to_string());
            let scope = args.scope.unwrap_or_else(|| "all".to_string());

            // Get the diff
            let diff_args: Vec<&str> = match scope.as_str() {
                "staged" => vec!["diff", "--cached", "--stat"],
                "unstaged" => vec!["diff", "--stat"],
                _ => vec!["diff", "HEAD", "--stat"],
            };

            let stats_result = tokio::process::Command::new("git")
                .args(&diff_args)
                .current_dir(&dir)
                .output()
                .await;

            let stats = match stats_result {
                Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                Err(e) => return Ok(DiffCheckOutput {
                    success: false,
                    diff: String::new(),
                    stats: String::new(),
                    error: Some(format!("Failed to run git diff: {e}")),
                }),
            };

            // Get the actual diff content (without stat)
            let diff_content_args: Vec<&str> = match scope.as_str() {
                "staged" => vec!["diff", "--cached"],
                "unstaged" => vec!["diff"],
                _ => vec!["diff", "HEAD"],
            };

            let diff_result = tokio::process::Command::new("git")
                .args(&diff_content_args)
                .current_dir(&dir)
                .output()
                .await;

            let diff = match diff_result {
                Ok(o) => {
                    let raw = String::from_utf8_lossy(&o.stdout);
                    signal::compress_output(&raw, Some(8000))
                }
                Err(e) => return Ok(DiffCheckOutput {
                    success: false,
                    diff: String::new(),
                    stats: String::new(),
                    error: Some(format!("Failed to run git diff: {e}")),
                }),
            };

            let has_changes = !stats.is_empty() && stats != "";

            Ok(DiffCheckOutput {
                success: true,
                diff,
                stats: if has_changes { stats } else { "No changes detected".into() },
                error: if has_changes { None } else { Some("No changes to diff".into()) },
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::Tool;

    #[test]
    fn test_run_tests_tool_definition() {
        let tool = RunTestsTool;
        assert_eq!(tool.name(), "run_tests");
    }

    #[test]
    fn test_check_output_tool_definition() {
        let tool = CheckOutputTool;
        assert_eq!(tool.name(), "check_output");
    }

    #[test]
    fn test_diff_check_tool_definition() {
        let tool = DiffCheckTool;
        assert_eq!(tool.name(), "diff_check");
    }

    #[tokio::test]
    async fn test_run_tests_no_project() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = RunTestsTool;
        let result = tool.call(RunTestsArgs { path: Some(tmp.path().to_string_lossy().to_string()) }).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("No recognized project type"));
    }

    #[tokio::test]
    async fn test_run_tests_rust_project() {
        let tool = RunTestsTool;
        // Use CARGO_MANIFEST_DIR to get the project root directory
        // Use cargo check instead of cargo test for faster testing
        let result = tool.call(RunTestsArgs { path: Some(env!("CARGO_MANIFEST_DIR").into()) }).await.unwrap();
        // The tool may fail if cargo test takes too long or has issues
        // We just verify it correctly detected the project type
        assert_eq!(result.test_runner, "cargo test");
    }

    #[tokio::test]
    async fn test_check_output_echo() {
        let tool = CheckOutputTool;
        let result = tool.call(CheckOutputArgs {
            command: "echo".into(),
            args: Some("hello world".into()),
            cwd: None,
            timeout: Some(5),
        }).await.unwrap();
        assert!(result.success);
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn test_check_output_nonexistent() {
        let tool = CheckOutputTool;
        let result = tool.call(CheckOutputArgs {
            command: "nonexistent_command_xyz".into(),
            args: None,
            cwd: None,
            timeout: Some(5),
        }).await.unwrap();
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_diff_check_in_git_repo() {
        let tool = DiffCheckTool;
        let result = tool.call(DiffCheckArgs {
            scope: Some("all".into()),
            path: Some(env!("CARGO_MANIFEST_DIR").into()),
        }).await.unwrap();
        assert!(result.success);
    }
}
