//! Command registry — extensible slash command system.
//!
//! Replaces the hardcoded if-else chain with a registration-based approach.
//! Commands are registered at startup with metadata (name, description, category, aliases).
//! Supports fuzzy matching, category filtering, and command history.

#![allow(dead_code)]

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Command category
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    Session,
    Agent,
    Tool,
    Skill,
    Config,
    Display,
}

impl CommandCategory {
    pub fn label(&self) -> &'static str {
        match self {
            CommandCategory::Session => "session",
            CommandCategory::Agent => "agent",
            CommandCategory::Tool => "tool",
            CommandCategory::Skill => "skill",
            CommandCategory::Config => "config",
            CommandCategory::Display => "display",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            CommandCategory::Session => "📂",
            CommandCategory::Agent => "🤖",
            CommandCategory::Tool => "🔧",
            CommandCategory::Skill => "⚡",
            CommandCategory::Config => "⚙️",
            CommandCategory::Display => "🎨",
        }
    }
}

// ---------------------------------------------------------------------------
// Command definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CommandDef {
    /// Primary command name (e.g. "help")
    pub name: &'static str,
    /// Short description for help text
    pub description: &'static str,
    /// Category for grouping
    pub category: CommandCategory,
    /// Alternative names (e.g. ["h", "?"])
    pub aliases: &'static [&'static str],
    /// Usage example
    pub usage: &'static str,
}

// ---------------------------------------------------------------------------
// Command registry
// ---------------------------------------------------------------------------

pub struct CommandRegistry {
    commands: Vec<CommandDef>,
    alias_map: HashMap<String, usize>,
    name_map: HashMap<String, usize>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: Vec::new(),
            alias_map: HashMap::new(),
            name_map: HashMap::new(),
        };
        registry.register_defaults();
        registry
    }

    /// Register a new command.
    pub fn register(&mut self, cmd: CommandDef) {
        let idx = self.commands.len();
        self.name_map.insert(cmd.name.to_string(), idx);
        for alias in cmd.aliases {
            self.alias_map.insert(alias.to_string(), idx);
        }
        self.commands.push(cmd);
    }

    /// Look up a command by name or alias.
    pub fn find(&self, name: &str) -> Option<&CommandDef> {
        // Try exact name match first
        if let Some(&idx) = self.name_map.get(name) {
            return Some(&self.commands[idx]);
        }
        // Try alias match
        if let Some(&idx) = self.alias_map.get(name) {
            return Some(&self.commands[idx]);
        }
        // Try fuzzy match (prefix)
        self.fuzzy_find(name)
    }

    /// Fuzzy find — prefix match with Levenshtein distance ≤ 2.
    fn fuzzy_find(&self, name: &str) -> Option<&CommandDef> {
        let lower = name.to_lowercase();
        let mut best: Option<(&CommandDef, usize)> = None;

        for cmd in &self.commands {
            // Check if name is a prefix
            if cmd.name.starts_with(&lower) {
                match &best {
                    None => best = Some((cmd, 0)),
                    Some((_, 0)) => {} // Keep exact prefix match
                    Some((_, d)) if d > &0 => best = Some((cmd, 0)),
                    _ => {}
                }
                continue;
            }

            // Levenshtein distance
            let dist = levenshtein_distance(&lower, cmd.name);
            if dist <= 2 {
                match &best {
                    None => best = Some((cmd, dist)),
                    Some((_, best_d)) if dist < *best_d => best = Some((cmd, dist)),
                    _ => {}
                }
            }
        }

        best.map(|(cmd, _)| cmd)
    }

    /// List all commands, optionally filtered by category.
    pub fn list(&self, category: Option<CommandCategory>) -> Vec<&CommandDef> {
        self.commands
            .iter()
            .filter(|cmd| category.map_or(true, |cat| cmd.category == cat))
            .collect()
    }

    /// Format help text for all commands.
    pub fn format_help(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();

        let categories = [
            CommandCategory::Session,
            CommandCategory::Agent,
            CommandCategory::Tool,
            CommandCategory::Skill,
            CommandCategory::Config,
            CommandCategory::Display,
        ];

        for cat in &categories {
            let cmds = self.list(Some(*cat));
            if cmds.is_empty() {
                continue;
            }
            writeln!(out, "  {} {}", cat.icon(), format!("{:?} Commands:", cat).bold()).ok();
            for cmd in cmds {
                let aliases_str = if cmd.aliases.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", cmd.aliases.join(", "))
                };
                writeln!(out, "    /{}{} — {}", cmd.name, aliases_str, cmd.description).ok();
            }
            writeln!(out).ok();
        }

        out
    }

    /// Register the default built-in commands.
    fn register_defaults(&mut self) {
        // Session commands
        self.register(CommandDef {
            name: "new",
            description: "Start a new session",
            category: CommandCategory::Session,
            aliases: &[],
            usage: "/new",
        });
        self.register(CommandDef {
            name: "sessions",
            description: "List all sessions",
            category: CommandCategory::Session,
            aliases: &["ls"],
            usage: "/sessions",
        });
        self.register(CommandDef {
            name: "switch",
            description: "Switch to session #n",
            category: CommandCategory::Session,
            aliases: &["s"],
            usage: "/switch <number>",
        });
        self.register(CommandDef {
            name: "compact",
            description: "Compress conversation history",
            category: CommandCategory::Session,
            aliases: &[],
            usage: "/compact",
        });

        // Agent commands
        self.register(CommandDef {
            name: "plan",
            description: "Enter plan mode for a task",
            category: CommandCategory::Agent,
            aliases: &[],
            usage: "/plan <your goal>",
        });
        self.register(CommandDef {
            name: "model",
            description: "Show or switch LLM model",
            category: CommandCategory::Agent,
            aliases: &["m"],
            usage: "/model [provider/model]",
        });
        self.register(CommandDef {
            name: "doctor",
            description: "Run diagnostics",
            category: CommandCategory::Agent,
            aliases: &[],
            usage: "/doctor",
        });

        // Config commands
        self.register(CommandDef {
            name: "config",
            description: "Set or get configuration values",
            category: CommandCategory::Config,
            aliases: &[],
            usage: "/config set <key> <value>",
        });

        // Display commands
        self.register(CommandDef {
            name: "theme",
            description: "Switch display theme",
            category: CommandCategory::Display,
            aliases: &["t"],
            usage: "/theme <name>",
        });
        self.register(CommandDef {
            name: "clear",
            description: "Clear the screen",
            category: CommandCategory::Display,
            aliases: &["cls"],
            usage: "/clear",
        });
        self.register(CommandDef {
            name: "help",
            description: "Show this help",
            category: CommandCategory::Display,
            aliases: &["h", "?"],
            usage: "/help",
        });
        self.register(CommandDef {
            name: "quit",
            description: "Exit rupoo",
            category: CommandCategory::Display,
            aliases: &["q", "exit"],
            usage: "/quit",
        });
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Levenshtein distance (simplified)
// ---------------------------------------------------------------------------

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 { return b_len; }
    if b_len == 0 { return a_len; }

    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[a_len][b_len]
}

// ---------------------------------------------------------------------------
// Bold helper (no dependency on owo-colors here)
// ---------------------------------------------------------------------------

trait BoldStr: Sized {
    fn bold(self) -> String;
}

impl BoldStr for String {
    fn bold(self) -> String {
        format!("\x1b[1m{}\x1b[0m", self)
    }
}

impl BoldStr for &str {
    fn bold(self) -> String {
        format!("\x1b[1m{}\x1b[0m", self)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_find_exact() {
        let registry = CommandRegistry::new();
        let cmd = registry.find("help");
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap().name, "help");
    }

    #[test]
    fn test_registry_find_alias() {
        let registry = CommandRegistry::new();
        let cmd = registry.find("q");
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap().name, "quit");
    }

    #[test]
    fn test_registry_fuzzy_match() {
        let registry = CommandRegistry::new();
        // "hel" should fuzzy-match to "help"
        let cmd = registry.find("hel");
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap().name, "help");
    }

    #[test]
    fn test_registry_list_category() {
        let registry = CommandRegistry::new();
        let session_cmds = registry.list(Some(CommandCategory::Session));
        assert!(!session_cmds.is_empty());
        assert!(session_cmds.iter().all(|c| c.category == CommandCategory::Session));
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein_distance("help", "help"), 0);
        assert_eq!(levenshtein_distance("help", "hel"), 1);
        assert_eq!(levenshtein_distance("help", "halp"), 1);
        assert_eq!(levenshtein_distance("plan", "pla"), 1);
    }

    #[test]
    fn test_format_help() {
        let registry = CommandRegistry::new();
        let help = registry.format_help();
        assert!(help.contains("help"));
        assert!(help.contains("quit"));
    }
}
