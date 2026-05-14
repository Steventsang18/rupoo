//! Git integration for auto-commit, status, and PR creation.
//! Uses `git2` for local operations and `gh` CLI for GitHub PRs.

use tracing::info;

use crate::error::{AgentError, AgentResult};

/// Wraps a git repository for programmatic operations.
pub struct GitRepo {
    repo: git2::Repository,
    #[allow(dead_code)]
    workdir: String,
}

/// A single status entry (modified file).
pub struct StatusEntry {
    pub path: String,
    pub status: String,
}

impl GitRepo {
    /// Open a git repository at or above the given path.
    pub fn open(path: &str) -> AgentResult<Self> {
        let repo = git2::Repository::discover(path)
            .map_err(|e| AgentError::Other(format!("not a git repository: {e}")))?;
        let workdir = repo
            .workdir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        info!(workdir = %workdir, "git repository opened");
        Ok(Self { repo, workdir })
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> AgentResult<String> {
        match self.repo.head() {
            Ok(head) => Ok(head
                .shorthand()
                .unwrap_or("(no branch)")
                .to_string()),
            Err(e) => {
                // Unborn branch (no commits yet) or detached HEAD
                // Try to read HEAD file directly for the branch name
                if let Ok(content) = std::fs::read_to_string(
                    self.repo.path().join("HEAD"),
                ) {
                    let trimmed = content.trim();
                    if let Some(branch) = trimmed
                        .strip_prefix("ref: refs/heads/")
                    {
                        return Ok(branch.trim().to_string());
                    }
                }
                Err(AgentError::Other(format!("failed to get HEAD: {e}")))
            }
        }
    }

    /// Check git status and return list of changed files.
    pub fn status(&self) -> AgentResult<Vec<StatusEntry>> {
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| AgentError::Other(format!("status error: {e}")))?;

        let entries: Vec<StatusEntry> = statuses
            .iter()
            .map(|entry| {
                let path = entry.path().unwrap_or("(unknown)").to_string();
                let status = describe_status(entry.status());
                StatusEntry { path, status }
            })
            .collect();

        Ok(entries)
    }

    /// Stage all changes and commit with the given message.
    /// Returns the commit hash (short).
    pub fn commit_all(&self, message: &str) -> AgentResult<String> {
        // Stage all changes
        let mut index = self
            .repo
            .index()
            .map_err(|e| AgentError::Other(format!("index error: {e}")))?;

        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| AgentError::Other(format!("add error: {e}")))?;

        index
            .write()
            .map_err(|e| AgentError::Other(format!("index write error: {e}")))?;

        let oid = index
            .write_tree()
            .map_err(|e| AgentError::Other(format!("tree write error: {e}")))?;

        let tree = self
            .repo
            .find_tree(oid)
            .map_err(|e| AgentError::Other(format!("find tree error: {e}")))?;

        // Get signature from git config
        let sig = self
            .repo
            .signature()
            .map_err(|e| AgentError::Other(format!("signature error: {e}")))?;

        // Get parent commit (HEAD)
        let parent = self.repo.head().ok().and_then(|h| h.peel_to_commit().ok());

        let parents: Vec<&git2::Commit> = parent.iter().collect();

        let commit_oid = self
            .repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .map_err(|e| AgentError::Other(format!("commit error: {e}")))?;

        let short_hash = &commit_oid.to_string()[..7];

        info!(
            hash = %short_hash,
            message = %message,
            files = self.status().ok().map_or(0, |s| s.len()),
            "git commit created"
        );

        Ok(short_hash.to_string())
    }

    /// Commit with a task ID reference in the message.
    pub fn commit_with_task_ref(
        &self,
        message: &str,
        task_id: &str,
    ) -> AgentResult<String> {
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
        .map_err(|e| AgentError::Other(format!("gh CLI not found: {e}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(body.as_bytes())
            .map_err(|e| AgentError::Other(format!("write stdin error: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| AgentError::Other(format!("gh pr create error: {e}")))?;

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    info!(url = %url, "GitHub PR created");
    Ok(url)
}

fn describe_status(flags: git2::Status) -> String {
    let mut parts = Vec::new();

    // CURRENT is defined as all-false (bits = 0).
    // contains(CURRENT) is always true (any & 0 == 0),
    // so we check bits() == 0 directly.
    if flags.bits() == 0 {
        return "clean".into();
    }

    if flags.contains(git2::Status::INDEX_NEW) {
        parts.push("staged_new");
    }
    if flags.contains(git2::Status::INDEX_MODIFIED) {
        parts.push("staged_modified");
    }
    if flags.contains(git2::Status::INDEX_DELETED) {
        parts.push("staged_deleted");
    }
    if flags.contains(git2::Status::WT_NEW) {
        parts.push("untracked");
    }
    if flags.contains(git2::Status::WT_MODIFIED) {
        parts.push("modified");
    }
    if flags.contains(git2::Status::WT_DELETED) {
        parts.push("deleted");
    }
    if parts.is_empty() {
        "unknown".into()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_describe_status() {
        // CURRENT (zero bits) must always return "clean"
        assert_eq!(describe_status(git2::Status::CURRENT), "clean");

        // Non-zero flags must produce non-clean output
        let s = describe_status(git2::Status::INDEX_NEW);
        assert_ne!(s, "clean", "INDEX_NEW should not be clean");
        assert!(s.contains("staged"), "INDEX_NEW should contain 'staged', got '{s}'");

        let s = describe_status(git2::Status::WT_MODIFIED);
        assert_ne!(s, "clean", "WT_MODIFIED should not be clean");
        assert!(s.contains("modified"), "WT_MODIFIED should contain 'modified', got '{s}'");
    }

    #[test]
    fn test_open_current_repo() {
        let result = GitRepo::open(".");
        match result {
            Ok(repo) => {
                // May fail on unborn branch, but open() itself succeeded
                let _branch = repo.current_branch().ok();
            }
            Err(e) => eprintln!("Not in git repo: {e}"),
        }
    }
}
