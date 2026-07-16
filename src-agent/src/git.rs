//! Git integration for auto-commit, status, and PR creation.
//!
//! Local operations go through the `git` CLI (via `std::process::Command`)
//! rather than `git2`. This keeps the binary free of the vendored OpenSSL /
//! libgit2 C dependencies — leaving a single TLS stack (rustls) in the
//! shipped artifact, which is both lighter and a smaller attack surface.

use std::path::Path;
use std::process::Command;
use tracing::info;

use crate::error::{AgentError, AgentResult};

/// Wraps a git repository for programmatic operations.
pub struct GitRepo {
    #[allow(dead_code)]
    workdir: String,
}

/// A single status entry (changed file).
pub struct StatusEntry {
    pub path: String,
    pub status: String,
}

/// Run `git <args>` with the given working directory; error if `git` is absent.
fn git_in(workdir: &str, args: &[&str]) -> AgentResult<std::process::Output> {
    let out = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .output()
        .map_err(|e| AgentError::Git(format!("git CLI not found: {e}")))?;
    Ok(out)
}

/// Run `git <args>` and return trimmed stdout, erroring on non-zero exit.
fn git_out(workdir: &str, args: &[&str]) -> AgentResult<String> {
    let out = git_in(workdir, args)?;
    if !out.status.success() {
        return Err(AgentError::Git(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl GitRepo {
    /// Open a git repository at or above the given path.
    pub fn open(path: &str) -> AgentResult<Self> {
        let out = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .map_err(|e| AgentError::Git(format!("git CLI not found: {e}")))?;
        if !out.status.success() {
            return Err(AgentError::Git(format!(
                "not a git repository: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let workdir = String::from_utf8_lossy(&out.stdout).trim().to_string();
        info!(workdir = %workdir, "git repository opened");
        Ok(Self { workdir })
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> AgentResult<String> {
        // `symbolic-ref` fails on detached/unborn HEAD — fall back to reading
        // the raw HEAD ref, exactly like the previous git2 path did.
        match git_out(&self.workdir, &["symbolic-ref", "--short", "HEAD"]) {
            Ok(b) if !b.is_empty() => Ok(b),
            _ => {
                let head_path = Path::new(&self.workdir).join(".git").join("HEAD");
                if let Ok(content) = std::fs::read_to_string(head_path) {
                    let trimmed = content.trim();
                    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                        return Ok(branch.trim().to_string());
                    }
                }
                Err(AgentError::Git("failed to get current branch".into()))
            }
        }
    }

    /// Check git status and return list of changed files.
    pub fn status(&self) -> AgentResult<Vec<StatusEntry>> {
        let out = git_in(&self.workdir, &["status", "--porcelain"])?;
        let mut entries = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if line.len() < 3 {
                continue;
            }
            // Two-char XY status prefix, then space + path.
            let x = line.as_bytes()[0] as char;
            let y = line.as_bytes()[1] as char;
            let raw_path = &line[3..];
            // Handle renames ("R  old -> new") by taking the new name.
            let path = if let Some(pos) = raw_path.find(" -> ") {
                raw_path[pos + 4..].to_string()
            } else {
                raw_path.to_string()
            };
            entries.push(StatusEntry {
                path,
                status: describe_status(x, y),
            });
        }
        Ok(entries)
    }

    /// Stage all changes and commit with the given message.
    /// Returns the short commit hash.
    pub fn commit_all(&self, message: &str) -> AgentResult<String> {
        // Stage everything (respects .gitignore, same as the prior add_all).
        git_out(&self.workdir, &["add", "-A"])?;
        // `git commit` reads committer/author from git config — the same
        // source git2::Repository::signature() used.
        git_out(&self.workdir, &["commit", "-q", "-m", message])?;
        let short = git_out(&self.workdir, &["rev-parse", "--short", "HEAD"])?;

        info!(
            hash = %short,
            message = %message,
            files = self.status().ok().map_or(0, |s| s.len()),
            "git commit created"
        );

        Ok(short)
    }

    /// Commit with a task ID reference in the message.
    pub fn commit_with_task_ref(&self, message: &str, task_id: &str) -> AgentResult<String> {
        let full_message = format!("[task:{task_id}] {message}");
        self.commit_all(&full_message)
    }
}

/// Create a GitHub PR using the `gh` CLI.
/// Returns the PR URL.
pub fn create_gh_pr(title: &str, body: &str) -> AgentResult<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("gh")
        .args(["pr", "create", "--title", title, "--body", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| AgentError::Git(format!("gh CLI not found: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(body.as_bytes())
            .map_err(|e| AgentError::Git(format!("write stdin error: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| AgentError::Git(format!("gh pr create error: {e}")))?;

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    info!(url = %url, "GitHub PR created");
    Ok(url)
}

/// Map a `git status --porcelain` XY pair to the same category vocabulary
/// the rest of the app already consumes.
fn describe_status(x: char, y: char) -> String {
    let mut parts = Vec::new();

    // Index (staged) status, `x`.
    match x {
        'A' => parts.push("staged_new"),
        'M' => parts.push("staged_modified"),
        'D' => parts.push("staged_deleted"),
        'R' => parts.push("staged_modified"),
        _ => {}
    }

    // Worktree status, `y`.
    match y {
        '?' => parts.push("untracked"),
        'M' => parts.push("modified"),
        'D' => parts.push("deleted"),
        'A' => parts.push("modified"),
        _ => {}
    }

    if parts.is_empty() {
        "clean".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_describe_status_clean() {
        assert_eq!(describe_status(' ', ' '), "clean");
    }

    #[test]
    fn test_describe_status_staged_new() {
        let s = describe_status('A', ' ');
        assert!(s.contains("staged"), "expected 'staged', got '{s}'");
        assert!(s.contains("new"), "expected 'new', got '{s}'");
        assert_ne!(s, "clean");
    }

    #[test]
    fn test_describe_status_modified() {
        let s = describe_status(' ', 'M');
        assert!(s.contains("modified"), "expected 'modified', got '{s}'");
        assert_ne!(s, "clean");
    }

    #[test]
    fn test_describe_status_untracked() {
        let s = describe_status('?', '?');
        assert!(s.contains("untracked"), "expected 'untracked', got '{s}'");
    }

    #[test]
    fn test_describe_status_deleted() {
        let s = describe_status('D', 'D');
        assert!(s.contains("staged_deleted"), "got '{s}'");
        assert!(s.contains("deleted"), "got '{s}'");
    }

    #[test]
    fn test_open_current_repo() {
        // Only meaningful inside a git worktree (dev / CI).
        if let Ok(repo) = GitRepo::open(".") {
            let _branch = repo.current_branch().ok();
        }
    }
}
