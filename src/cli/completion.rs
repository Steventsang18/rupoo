//! Command completion for Rupoo CLI
//!
//! Provides intelligent tab completion for commands, file paths, and tools.

use rustyline::highlight::CmdKind;
use rustyline::history::FileHistory;
use rustyline::{
    completion::{Completer, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::Hinter,
    validate::Validator,
    Context, Helper,
};
use std::path::Path;

/// Combined helper struct implementing all required traits
#[derive(Clone)]
pub struct RupooHelper;

impl Completer for RupooHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let (start, candidates) = if line.starts_with('/') {
            // Command completion
            let cmd_part = &line[1..pos];
            let commands = [
                "help", "h", "?", "tools", "ts", "new", "sessions", "ls", "switch", "s", "model",
                "m", "theme", "t", "plan", "clear", "cls", "quit", "q", "exit", "history", "alias",
            ];
            let matches: Vec<_> = commands
                .iter()
                .filter(|c| c.starts_with(cmd_part))
                .map(|c| Pair {
                    display: format!("/{}", c),
                    replacement: format!("/{} ", c),
                })
                .collect();
            (1, matches)
        } else if line.starts_with('@') {
            // File path completion
            let path_part = &line[1..pos];
            (1, Self::complete_path(path_part))
        } else if line.starts_with('!') {
            // Shell command completion
            let cmd_part = &line[1..pos];
            let common_cmds = ["ls", "cd", "cat", "grep", "find", "git", "cargo", "npm"];
            let matches: Vec<_> = common_cmds
                .iter()
                .filter(|c| c.starts_with(cmd_part))
                .map(|c| Pair {
                    display: format!("!{}", c),
                    replacement: format!("!{} ", c),
                })
                .collect();
            (1, matches)
        } else {
            (pos, Vec::new())
        };

        Ok((start, candidates))
    }
}

impl RupooHelper {
    fn complete_path(path_part: &str) -> Vec<Pair> {
        use std::fs;

        let path = Path::new(path_part);
        let (dir, prefix) = if path.has_root() {
            if let Some(parent) = path.parent() {
                (
                    parent.to_path_buf(),
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string(),
                )
            } else {
                (path.to_path_buf(), "".to_string())
            }
        } else {
            let cwd = match std::env::current_dir() {
                Ok(d) => d,
                Err(_) => return Vec::new(),
            };
            let full_path = cwd.join(path);
            let dir = if let Some(parent) = full_path.parent() {
                parent.to_path_buf()
            } else {
                cwd.clone()
            };
            let prefix = full_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(path_part)
                .to_string();
            (dir, prefix)
        };

        let mut results = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                if let Some(name) = file_name.to_str() {
                    if name.starts_with(&prefix) {
                        let full_path = dir.join(name);
                        let display_name = if full_path.is_dir() {
                            format!("{}/", name)
                        } else {
                            name.to_string()
                        };
                        results.push(Pair {
                            display: display_name.clone(),
                            replacement: display_name,
                        });
                    }
                }
            }
        }
        results.sort_by(|a, b| a.display.cmp(&b.display));
        results
    }
}

impl Highlighter for RupooHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _is_done: bool,
    ) -> std::borrow::Cow<'b, str> {
        use owo_colors::OwoColorize;
        std::borrow::Cow::Owned(prompt.green().bold().to_string())
    }

    fn highlight<'b>(&self, line: &'b str, _pos: usize) -> std::borrow::Cow<'b, str> {
        use owo_colors::OwoColorize;

        if line.starts_with('/') {
            std::borrow::Cow::Owned(line.cyan().to_string())
        } else if line.starts_with('@') {
            std::borrow::Cow::Owned(line.yellow().to_string())
        } else if line.starts_with('!') {
            std::borrow::Cow::Owned(line.red().to_string())
        } else {
            std::borrow::Cow::Borrowed(line)
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _ctx: CmdKind) -> bool {
        true
    }
}

impl Hinter for RupooHelper {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Validator for RupooHelper {}

impl Helper for RupooHelper {}

/// Helper function to create a configured editor with completion
pub fn create_editor() -> Result<rustyline::Editor<RupooHelper, FileHistory>, ReadlineError> {
    use rustyline::config::Behavior;

    // Create editor with config that properly handles SIGINT
    let config = rustyline::Config::builder()
        .behavior(Behavior::PreferTerm) // Use terminal mode for better signal handling
        .edit_mode(rustyline::EditMode::Emacs)
        .max_history_size(1000)?
        .build();

    let mut rl = rustyline::Editor::with_config(config)?;

    // Configure completion
    let helper = RupooHelper;
    rl.set_helper(Some(helper));

    Ok(rl)
}
