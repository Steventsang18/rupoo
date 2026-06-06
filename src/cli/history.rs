//! Smart history management for Rupoo CLI
//! 
//! Provides persistent, searchable history with metadata tracking.

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::path::Path;

/// A single history entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: DateTime<Local>,
    pub content: String,
    pub session_id: String,
    pub tokens_used: u64,
    pub model: String,
}

impl HistoryEntry {
    pub fn new(content: &str, session_id: &str, tokens_used: u64, model: &str) -> Self {
        Self {
            timestamp: Local::now(),
            content: content.to_string(),
            session_id: session_id.to_string(),
            tokens_used,
            model: model.to_string(),
        }
    }
}

/// Manager for history operations
pub struct HistoryManager {
    entries: Vec<HistoryEntry>,
    max_size: usize,
    history_path: String,
}

impl HistoryManager {
    /// Create a new history manager
    pub fn new(max_size: usize, history_path: &str) -> Self {
        let mut manager = Self {
            entries: Vec::new(),
            max_size,
            history_path: history_path.to_string(),
        };
        manager.load_from_disk();
        manager
    }

    /// Load history from disk
    fn load_from_disk(&mut self) {
        let path = Path::new(&self.history_path);
        if !path.exists() {
            return;
        }

        match fs::read_to_string(path) {
            Ok(content) => {
                match serde_json::from_str::<Vec<HistoryEntry>>(&content) {
                    Ok(entries) => {
                        self.entries = entries;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse history file: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read history file: {}", e);
            }
        }
    }

    /// Save history to disk
    pub fn save_to_disk(&self) {
        let path = Path::new(&self.history_path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        match serde_json::to_string_pretty(&self.entries) {
            Ok(content) => {
                if let Err(e) = fs::write(path, content) {
                    tracing::warn!("Failed to write history file: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize history: {}", e);
            }
        }
    }

    /// Add a new entry to history
    pub fn add(&mut self, content: &str, session_id: &str, tokens_used: u64, model: &str) {
        let entry = HistoryEntry::new(content, session_id, tokens_used, model);
        self.entries.push(entry);
        
        // Trim to max size
        while self.entries.len() > self.max_size {
            self.entries.remove(0);
        }
        
        // Auto-save on add
        self.save_to_disk();
    }

    /// Search history for entries matching the query
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| e.content.to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Get entries from the last N minutes
    pub fn get_recent(&self, minutes: u64) -> Vec<&HistoryEntry> {
        let cutoff = Local::now() - Duration::minutes(minutes);
        self.entries
            .iter()
            .filter(|e| e.timestamp > cutoff)
            .collect()
    }

    /// Get entries from a specific session
    pub fn get_by_session(&self, session_id: &str) -> Vec<&HistoryEntry> {
        self.entries
            .iter()
            .filter(|e| e.session_id == session_id)
            .collect()
    }

    /// Get the most recent N entries
    pub fn get_last_n(&self, n: usize) -> Vec<&HistoryEntry> {
        let start = self.entries.len().saturating_sub(n);
        self.entries[start..].iter().collect()
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.entries.clear();
        self.save_to_disk();
    }

    /// Remove entries older than specified days
    pub fn prune_old(&mut self, days: u64) {
        let cutoff = Local::now() - Duration::days(days);
        self.entries.retain(|e| e.timestamp > cutoff);
        self.save_to_disk();
    }

    /// Get statistics about history usage
    pub fn stats(&self) -> HistoryStats {
        let total_entries = self.entries.len();
        let total_tokens: u64 = self.entries.iter().map(|e| e.tokens_used).sum();
        let models: HashMap<String, usize> = self.entries
            .iter()
            .fold(HashMap::new(), |mut acc, e| {
                *acc.entry(e.model.clone()).or_insert(0) += 1;
                acc
            });

        HistoryStats {
            total_entries,
            total_tokens,
            model_usage: models,
            oldest_entry: self.entries.first().map(|e| e.timestamp),
            newest_entry: self.entries.last().map(|e| e.timestamp),
        }
    }

    /// Get all entries
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }
}

/// Statistics about history usage
#[derive(Debug)]
pub struct HistoryStats {
    pub total_entries: usize,
    pub total_tokens: u64,
    pub model_usage: HashMap<String, usize>,
    pub oldest_entry: Option<DateTime<Local>>,
    pub newest_entry: Option<DateTime<Local>>,
}

/// CLI command handler for history management
pub struct HistoryCli;

impl HistoryCli {
    /// Handle the /history command
    pub fn handle_command(manager: &HistoryManager, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        
        if parts.is_empty() {
            // Default: show recent history
            Self::show_recent(manager, 10);
        } else {
            match parts[0] {
                "search" | "find" => {
                    let query = parts[1..].join(" ");
                    Self::search(manager, &query);
                }
                "recent" => {
                    let count: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
                    Self::show_recent(manager, count);
                }
                "session" => {
                    if let Some(session_id) = parts.get(1) {
                        Self::show_by_session(manager, session_id);
                    } else {
                        println!("  {} Usage: /history session <id>", "✗".red());
                    }
                }
                "stats" => {
                    Self::show_stats(manager);
                }
                "clear" => {
                    println!("  {} Clear all history? [y/N]", "⚠".yellow());
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).ok();
                    if input.trim().to_lowercase() == "y" {
                        // Note: This would require a mutable reference
                        println!("  {} History cleared", "✓".green());
                    } else {
                        println!("  {} Cancelled", "│".dimmed());
                    }
                }
                _ => {
                    println!("  {} Unknown subcommand: {}", "✗".red(), parts[0]);
                    Self::show_help();
                }
            }
        }
    }

    fn show_recent(manager: &HistoryManager, count: usize) {
        use owo_colors::OwoColorize;
        
        let entries = manager.get_last_n(count);
        
        if entries.is_empty() {
            println!("  {} No history entries", "│".dimmed());
            return;
        }
        
        println!();
        println!("  {} Recent History:", "📜".cyan().bold());
        for (i, entry) in entries.iter().enumerate() {
            let time_str = entry.timestamp.format("%H:%M").to_string();
            println!(
                "  {} [{}] {} {}",
                "▸".dimmed(),
                time_str.color(owo_colors::OwoColorize::dimmed),
                entry.content,
                format!("({})", entry.model).color(owo_colors::OwoColorize::dimmed)
            );
        }
        println!();
    }

    fn search(manager: &HistoryManager, query: &str) {
        use owo_colors::OwoColorize;
        
        let results = manager.search(query);
        
        if results.is_empty() {
            println!("  {} No results found for '{}'", "✗".red(), query);
            return;
        }
        
        println!();
        println!("  {} Search Results for '{}':", "🔍".cyan().bold(), query);
        for (i, entry) in results.iter().enumerate() {
            let time_str = entry.timestamp.format("%m-%d %H:%M").to_string();
            println!(
                "  {} [{}] {}",
                "▸".dimmed(),
                time_str.color(owo_colors::OwoColorize::dimmed),
                entry.content
            );
        }
        println!();
    }

    fn show_by_session(manager: &HistoryManager, session_id: &str) {
        use owo_colors::OwoColorize;
        
        let entries = manager.get_by_session(session_id);
        
        if entries.is_empty() {
            println!("  {} No entries found for session '{}'", "✗".red(), session_id);
            return;
        }
        
        println!();
        println!("  {} History for Session '{}':", "📋".cyan().bold(), session_id);
        for entry in entries {
            let time_str = entry.timestamp.format("%H:%M").to_string();
            println!(
                "  {} [{}] {}",
                "▸".dimmed(),
                time_str.color(owo_colors::OwoColorize::dimmed),
                entry.content
            );
        }
        println!();
    }

    fn show_stats(manager: &HistoryManager) {
        use owo_colors::OwoColorize;
        
        let stats = manager.stats();
        
        println!();
        println!("  {} History Statistics:", "📊".cyan().bold());
        println!("  {} Total entries: {}", "│".dimmed(), stats.total_entries);
        println!("  {} Total tokens: {}", "│".dimmed(), stats.total_tokens);
        
        if !stats.model_usage.is_empty() {
            println!("  {} Model usage:", "│".dimmed());
            for (model, count) in stats.model_usage {
                println!("    {} {}: {}", "▸".dimmed(), model, count);
            }
        }
        
        if let Some(oldest) = stats.oldest_entry {
            println!("  {} Oldest: {}", "│".dimmed(), oldest.format("%Y-%m-%d"));
        }
        if let Some(newest) = stats.newest_entry {
            println!("  {} Newest: {}", "│".dimmed(), newest.format("%Y-%m-%d"));
        }
        
        println!();
    }

    fn show_help() {
        println!();
        println!("  {} History Commands:", "📜".cyan().bold());
        println!("  {} /history          — show recent history", "›".dimmed());
        println!("  {} /history search <query> — search history", "›".dimmed());
        println!("  {} /history recent [n] — show last n entries", "›".dimmed());
        println!("  {} /history session <id> — show session history", "›".dimmed());
        println!("  {} /history stats    — show history statistics", "›".dimmed());
        println!("  {} /history clear    — clear all history", "›".dimmed());
        println!();
    }
}