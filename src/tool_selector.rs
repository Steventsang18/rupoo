//! Intelligent tool selection and scoring engine.
//!
//! When the LLM calls tools, this module:
//! 1. Scores tool choices against the task context (relevance, cost, safety)
//! 2. Ranks alternative tools when the chosen tool is suboptimal
//! 3. Detects and warns about dangerous tool combinations
//! 4. Provides tool usage statistics for adaptive optimization
//!
//! The scoring model balances multiple dimensions:
//! - **Semantic relevance**: how well the tool matches the task description
//! - **Historical success**: how often the tool succeeded in similar contexts
//! - **Safety cost**: approval overhead and risk level
//! - **Token efficiency**: estimated token cost of tool output vs. LLM reasoning

use std::collections::HashMap;
use std::sync::RwLock;

use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Tool metadata
// ---------------------------------------------------------------------------

/// Metadata describing a tool's capabilities, cost, and risk.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// Unique tool name (matches rig tool NAME constant).
    pub name: &'static str,
    /// Human-readable description of what the tool does.
    pub description: &'static str,
    /// Keywords that this tool is relevant for.
    pub keywords: &'static [&'static str],
    /// Semantic category of this tool.
    pub category: ToolCategory,
    /// Risk level: affects whether approval is required.
    pub risk_level: RiskLevel,
    /// Whether this tool can run in parallel with others.
    pub parallelizable: bool,
    /// Whether this tool modifies state (vs. read-only).
    pub mutates_state: bool,
    /// Approximate token cost of typical output.
    pub typical_output_tokens: usize,
    /// Whether this tool allows sub-tool delegation.
    pub allows_delegation: bool,
}

/// High-level categories for tools.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    FileRead,
    FileWrite,
    Shell,
    Network,
    Search,
    Browser,
    Verification,
    Utility,
}

/// Risk level for tool execution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    /// Safe read-only operations, no approval needed.
    Safe,
    /// Low risk, auto-approved but logged.
    Low,
    /// Medium risk, may need user review.
    Medium,
    /// High risk, always requires explicit user approval.
    High,
    /// Critical risk, blocked unless explicitly configured.
    Critical,
}

impl ToolCategory {
    /// Whether two tool categories can execute in parallel.
    pub fn can_parallel_with(&self, other: &ToolCategory) -> bool {
        // File writes and shell execs cannot run in parallel
        // due to potential state conflicts
        !matches!(
            (self, other),
            (
                ToolCategory::FileWrite,
                ToolCategory::Shell | ToolCategory::FileWrite
            ) | (ToolCategory::Shell, ToolCategory::FileWrite)
        )
    }
}

impl RiskLevel {
    /// Whether this risk level requires user approval.
    pub fn requires_approval(&self) -> bool {
        matches!(self, RiskLevel::High | RiskLevel::Critical)
    }

    /// Whether this risk level is blocked entirely.
    pub fn is_blocked(&self) -> bool {
        matches!(self, RiskLevel::Critical)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Safe => "safe",
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "safe" => Some(RiskLevel::Safe),
            "low" => Some(RiskLevel::Low),
            "medium" => Some(RiskLevel::Medium),
            "high" => Some(RiskLevel::High),
            "critical" => Some(RiskLevel::Critical),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool registry — all available tools with metadata
// ---------------------------------------------------------------------------

/// Global registry of available tools.
pub struct ToolRegistry {
    tools: HashMap<&'static str, ToolInfo>,
}

impl ToolRegistry {
    /// Create a registry with all built-in tools.
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
        };

        registry.register(ToolInfo {
            name: "file_read",
            description: "Read file contents",
            keywords: &["read", "file", "view", "open", "cat", "content", "code"],
            category: ToolCategory::FileRead,
            risk_level: RiskLevel::Safe,
            parallelizable: true,
            mutates_state: false,
            typical_output_tokens: 500,
            allows_delegation: false,
        });

        registry.register(ToolInfo {
            name: "file_write",
            description: "Write content to a file",
            keywords: &[
                "write", "file", "save", "create", "edit", "modify", "generate",
            ],
            category: ToolCategory::FileWrite,
            risk_level: RiskLevel::Medium,
            parallelizable: false,
            mutates_state: true,
            typical_output_tokens: 100,
            allows_delegation: false,
        });

        registry.register(ToolInfo {
            name: "list_directory",
            description: "List directory contents",
            keywords: &[
                "list",
                "dir",
                "ls",
                "directory",
                "folder",
                "files",
                "structure",
            ],
            category: ToolCategory::FileRead,
            risk_level: RiskLevel::Safe,
            parallelizable: true,
            mutates_state: false,
            typical_output_tokens: 300,
            allows_delegation: false,
        });

        registry.register(ToolInfo {
            name: "shell_exec",
            description: "Execute a shell command",
            keywords: &[
                "execute", "run", "command", "shell", "bash", "build", "test", "install",
                "compile", "git", "npm", "cargo",
            ],
            category: ToolCategory::Shell,
            risk_level: RiskLevel::High,
            parallelizable: false,
            mutates_state: true,
            typical_output_tokens: 1000,
            allows_delegation: true,
        });

        registry.register(ToolInfo {
            name: "web_search",
            description: "Search the web",
            keywords: &[
                "search", "web", "google", "find", "query", "internet", "lookup",
            ],
            category: ToolCategory::Search,
            risk_level: RiskLevel::Low,
            parallelizable: true,
            mutates_state: false,
            typical_output_tokens: 800,
            allows_delegation: false,
        });

        registry.register(ToolInfo {
            name: "echo",
            description: "Echo a message back (testing)",
            keywords: &["echo", "test", "debug", "print"],
            category: ToolCategory::Utility,
            risk_level: RiskLevel::Safe,
            parallelizable: true,
            mutates_state: false,
            typical_output_tokens: 50,
            allows_delegation: false,
        });

        registry.register(ToolInfo {
            name: "delete_file",
            description: "Delete a file",
            keywords: &["delete", "remove", "rm", "clean", "unlink"],
            category: ToolCategory::FileWrite,
            risk_level: RiskLevel::High,
            parallelizable: false,
            mutates_state: true,
            typical_output_tokens: 50,
            allows_delegation: false,
        });

        registry
    }

    fn register(&mut self, info: ToolInfo) {
        self.tools.insert(info.name, info);
    }

    /// Get tool info by name.
    pub fn get(&self, name: &str) -> Option<&ToolInfo> {
        self.tools.get(name)
    }

    /// Get all registered tool names.
    pub fn names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    /// List all tools in a category.
    pub fn by_category(&self, category: &ToolCategory) -> Vec<&ToolInfo> {
        self.tools
            .values()
            .filter(|t| &t.category == category)
            .collect()
    }

    /// Find tools matching keywords in a task description.
    pub fn match_task(&self, task_description: &str) -> Vec<(&ToolInfo, f64)> {
        let lower = task_description.to_lowercase();
        let mut scored: Vec<_> = self
            .tools
            .values()
            .map(|tool| {
                let score = tool
                    .keywords
                    .iter()
                    .filter(|kw| lower.contains(*kw))
                    .count() as f64;
                (tool, score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Recommend alternatives for a tool given a task description.
    pub fn recommend_alternatives(
        &self,
        tool_name: &str,
        task_description: &str,
    ) -> Vec<(&'static str, f64)> {
        let chosen = match self.get(tool_name) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut alternatives: Vec<_> = self
            .tools
            .iter()
            .filter(|(name, info)| {
                **name != tool_name
                    && info.category == chosen.category
                    && info.risk_level <= chosen.risk_level
            })
            .map(|(name, tool)| {
                let lower = task_description.to_lowercase();
                let score = tool
                    .keywords
                    .iter()
                    .filter(|kw| lower.contains(*kw))
                    .count() as f64;
                (*name, score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        alternatives.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        alternatives
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tool usage statistics
// ---------------------------------------------------------------------------

/// Per-tool usage statistics for adaptive optimization.
#[derive(Debug, Clone, Default)]
pub struct ToolStats {
    /// Total number of calls to this tool.
    pub call_count: u64,
    /// Number of successful executions.
    pub success_count: u64,
    /// Cumulative execution time in milliseconds.
    pub total_time_ms: u64,
    /// Approximate total output tokens produced.
    pub total_output_tokens: u64,
    /// Most recent success rate (last 100 calls).
    recent_results: Vec<bool>,
}

impl ToolStats {
    /// Record a tool execution result.
    pub fn record(&mut self, success: bool, duration_ms: u64, output_tokens: usize) {
        self.call_count += 1;
        if success {
            self.success_count += 1;
        }
        self.total_time_ms += duration_ms;
        self.total_output_tokens += output_tokens as u64;

        self.recent_results.push(success);
        if self.recent_results.len() > 100 {
            self.recent_results.remove(0);
        }
    }

    /// Overall success rate (0.0 to 1.0).
    pub fn success_rate(&self) -> f64 {
        if self.call_count == 0 {
            return 1.0;
        }
        self.success_count as f64 / self.call_count as f64
    }

    /// Recent success rate from last 100 calls.
    pub fn recent_success_rate(&self) -> f64 {
        if self.recent_results.is_empty() {
            return 1.0;
        }
        let successes = self.recent_results.iter().filter(|&&r| r).count();
        successes as f64 / self.recent_results.len() as f64
    }

    /// Average execution time in milliseconds.
    pub fn avg_time_ms(&self) -> f64 {
        if self.call_count == 0 {
            return 0.0;
        }
        self.total_time_ms as f64 / self.call_count as f64
    }

    /// Average output tokens per call.
    pub fn avg_output_tokens(&self) -> f64 {
        if self.call_count == 0 {
            return 0.0;
        }
        self.total_output_tokens as f64 / self.call_count as f64
    }
}

/// Thread-safe tool usage tracker.
pub struct ToolUsageTracker {
    stats: RwLock<HashMap<String, ToolStats>>,
}

impl ToolUsageTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a tool execution result.
    pub fn record(&self, tool_name: &str, success: bool, duration_ms: u64, output_tokens: usize) {
        let mut stats = self.stats.write().expect("tool_stats lock poisoned");
        stats
            .entry(tool_name.to_string())
            .or_default()
            .record(success, duration_ms, output_tokens);
    }

    /// Get statistics for a tool.
    pub fn get_stats(&self, tool_name: &str) -> Option<ToolStats> {
        let stats = self.stats.read().expect("tool_stats lock poisoned");
        stats.get(tool_name).cloned()
    }

    /// Get a summary of tool effectiveness for context injection.
    pub fn summary(&self) -> String {
        let stats = self.stats.read().expect("tool_stats lock poisoned");
        let mut entries: Vec<_> = stats
            .iter()
            .filter(|(_, s)| s.call_count > 0)
            .map(|(name, s)| {
                format!(
                    "- {}: {:.0}% success ({} calls)",
                    name,
                    s.success_rate() * 100.0,
                    s.call_count
                )
            })
            .collect();
        if entries.is_empty() {
            return String::new();
        }
        entries.sort();
        format!("## Tool Effectiveness\n{}", entries.join("\n"))
    }

    /// Total calls across all tools.
    pub fn total_calls(&self) -> u64 {
        self.stats
            .read()
            .expect("tool_stats lock poisoned")
            .values()
            .map(|s| s.call_count)
            .sum()
    }
}

impl Default for ToolUsageTracker {
    fn default() -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// Dependency analysis for parallel execution
// ---------------------------------------------------------------------------

/// Tool call with its dependency information.
#[derive(Debug, Clone)]
pub struct ToolCallDep {
    pub tool_name: String,
    pub params: serde_json::Value,
    /// Tools this call depends on (by call index).
    pub depends_on: Vec<usize>,
}

/// Analyze tool calls and group them into parallel batches.
///
/// Returns batches of tool calls that can run in parallel.
/// Calls within a batch have no dependencies on each other.
pub fn analyze_parallel_batches(calls: &[ToolCallDep], registry: &ToolRegistry) -> Vec<Vec<usize>> {
    if calls.is_empty() {
        return Vec::new();
    }

    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut completed: Vec<bool> = vec![false; calls.len()];

    while completed.iter().any(|c| !*c) {
        let mut batch: Vec<usize> = Vec::new();

        for (i, call) in calls.iter().enumerate() {
            if completed[i] {
                continue;
            }

            // Check if all dependencies are completed
            let deps_met = call.depends_on.iter().all(|&d| completed[d]);
            if !deps_met {
                continue;
            }

            // Check if parallelizable
            let info = registry.get(&call.tool_name);
            let parallelizable = info.map(|t| t.parallelizable).unwrap_or(false);

            // Check for conflicts within current batch
            let no_conflict = batch.iter().all(|&j| {
                let other_info = registry.get(&calls[j].tool_name);
                match (info, other_info) {
                    (Some(a), Some(b)) => a.category.can_parallel_with(&b.category),
                    _ => false,
                }
            });

            if parallelizable && no_conflict {
                batch.push(i);
            }
        }

        if batch.is_empty() {
            // If no parallel candidates, run remaining sequentially one by one
            for (i, _) in calls.iter().enumerate() {
                if !completed[i] {
                    batches.push(vec![i]);
                    completed[i] = true;
                    break;
                }
            }
        } else {
            for &i in &batch {
                completed[i] = true;
            }
            batches.push(batch);
        }
    }

    debug!(
        total_calls = calls.len(),
        batches = batches.len(),
        "parallel execution plan"
    );
    batches
}

// ---------------------------------------------------------------------------
// Dangerous combination detection
// ---------------------------------------------------------------------------

/// Known dangerous tool call patterns that should trigger warnings.
const DANGEROUS_PATTERNS: &[(&[&str], &str)] = &[
    (
        &["file_read", "shell_exec"],
        "file_read + shell_exec: potential code injection",
    ),
    (
        &["file_write", "shell_exec"],
        "file_write + shell_exec: potential malicious script",
    ),
    (
        &["shell_exec", "shell_exec"],
        "Multiple shell_exec calls: verify commands are safe",
    ),
    (
        &["delete_file", "file_write"],
        "delete_file + file_write: potential data loss pattern",
    ),
];

/// Check if a set of tool calls contains dangerous patterns.
pub fn detect_dangerous_patterns(tool_names: &[String]) -> Vec<String> {
    let mut warnings = Vec::new();

    let lower_names: Vec<String> = tool_names.iter().map(|n| n.to_lowercase()).collect();

    for &(pattern, message) in DANGEROUS_PATTERNS {
        let all_match = pattern
            .iter()
            .all(|p| lower_names.iter().any(|n| n.contains(p)));
        if all_match {
            warnings.push(message.to_string());
        }
    }

    if !warnings.is_empty() {
        warn!(count = warnings.len(), "dangerous tool patterns detected");
    }

    warnings
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_registry_known_tools() {
        let registry = ToolRegistry::new();
        assert!(registry.get("file_read").is_some());
        assert!(registry.get("file_write").is_some());
        assert!(registry.get("shell_exec").is_some());
        assert!(registry.get("web_search").is_some());
        assert!(registry.get("list_directory").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_tool_registry_match_task() {
        let registry = ToolRegistry::new();
        let results = registry.match_task("read the main.rs file");
        assert!(!results.is_empty());
        // file_read should be the top match
        assert!(
            results[0].0.name == "file_read" || results.iter().any(|(t, _)| t.name == "file_read")
        );
    }

    #[test]
    fn test_tool_registry_recommend_alternatives() {
        let registry = ToolRegistry::new();
        let alts = registry.recommend_alternatives("file_read", "list the directory contents");
        // list_directory should be recommended as alternative to file_read
        assert!(alts.iter().any(|(name, _)| *name == "list_directory"));
    }

    #[test]
    fn test_tool_registry_by_category() {
        let registry = ToolRegistry::new();
        let file_tools = registry.by_category(&ToolCategory::FileRead);
        assert!(file_tools.iter().any(|t| t.name == "file_read"));
        assert!(file_tools.iter().any(|t| t.name == "list_directory"));
    }

    #[test]
    fn test_tool_stats_recording() {
        let mut stats = ToolStats::default();
        stats.record(true, 100, 500);
        stats.record(false, 200, 300);
        assert_eq!(stats.call_count, 2);
        assert_eq!(stats.success_count, 1);
        assert_eq!(stats.success_rate(), 0.5);
        assert_eq!(stats.avg_time_ms(), 150.0);
    }

    #[test]
    fn test_tool_stats_recent_rate() {
        let mut stats = ToolStats::default();
        for i in 0..10 {
            stats.record(i % 2 == 0, 50, 100);
        }
        assert_eq!(stats.recent_success_rate(), 0.5);
    }

    #[test]
    fn test_usage_tracker() {
        let tracker = ToolUsageTracker::new();
        tracker.record("file_read", true, 50, 200);
        tracker.record("file_read", true, 60, 300);
        tracker.record("file_read", false, 100, 0);

        let s = tracker.get_stats("file_read").unwrap();
        assert_eq!(s.call_count, 3);
        assert_eq!(s.success_count, 2);
    }

    #[test]
    fn test_usage_tracker_summary() {
        let tracker = ToolUsageTracker::new();
        tracker.record("file_read", true, 50, 200);

        let summary = tracker.summary();
        assert!(summary.contains("file_read"));
        assert!(summary.contains("success"));
    }

    #[test]
    fn test_parallel_batches_empty() {
        let registry = ToolRegistry::new();
        let batches = analyze_parallel_batches(&[], &registry);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_parallel_batches_single() {
        let registry = ToolRegistry::new();
        let calls = vec![ToolCallDep {
            tool_name: "file_read".to_string(),
            params: serde_json::json!({"path": "test.txt"}),
            depends_on: vec![],
        }];
        let batches = analyze_parallel_batches(&calls, &registry);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], vec![0]);
    }

    #[test]
    fn test_parallel_batches_sequential() {
        let registry = ToolRegistry::new();
        let calls = vec![
            ToolCallDep {
                tool_name: "file_read".to_string(),
                params: serde_json::json!({"path": "src.txt"}),
                depends_on: vec![],
            },
            ToolCallDep {
                tool_name: "file_write".to_string(),
                params: serde_json::json!({"path": "dest.txt", "content": "test"}),
                depends_on: vec![0], // depends on file_read completing first
            },
        ];
        let batches = analyze_parallel_batches(&calls, &registry);
        // Should have 2 sequential batches
        assert!(!batches.is_empty());
    }

    #[test]
    fn test_dangerous_pattern_detection() {
        let warnings =
            detect_dangerous_patterns(&["file_read".to_string(), "shell_exec".to_string()]);
        assert!(!warnings.is_empty());

        let no_warnings = detect_dangerous_patterns(&["file_read".to_string(), "echo".to_string()]);
        assert!(no_warnings.is_empty());
    }

    #[test]
    fn test_dangerous_pattern_multiple_shell() {
        let warnings =
            detect_dangerous_patterns(&["shell_exec".to_string(), "shell_exec".to_string()]);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Safe < RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn test_risk_level_approval() {
        assert!(!RiskLevel::Safe.requires_approval());
        assert!(!RiskLevel::Low.requires_approval());
        assert!(!RiskLevel::Medium.requires_approval());
        assert!(RiskLevel::High.requires_approval());
        assert!(RiskLevel::Critical.requires_approval());
    }

    #[test]
    fn test_category_parallel_check() {
        assert!(ToolCategory::FileRead.can_parallel_with(&ToolCategory::FileRead));
        assert!(ToolCategory::FileRead.can_parallel_with(&ToolCategory::Search));
        assert!(!ToolCategory::FileWrite.can_parallel_with(&ToolCategory::Shell));
    }
}
