//! Step progress display for the REPL.
//!
//! Uses ANSI escape codes to update terminal lines in-place,
//! showing step indicators that transition from running → done/failed.
//!
//! Example output:
//!
//!   ✓ Analyzing module structure                          (1.2s)
//!   ✓ Adding dependency to Cargo.toml                     (0.4s)
//!   ● Running cargo test...                               (2.3s)
//!     └─ Compiling 48 crates...

use std::io::Write;
use std::time::Instant;

use console::style;

/// Tracks and renders step progress in the terminal.
pub struct StepProgress {
    /// Rendered lines for completed steps.
    completed: Vec<String>,
    /// Current running step, if any.
    running: Option<RunningStep>,
}

struct RunningStep {
    icon: &'static str,
    label: String,
    start: Instant,
    details: Vec<String>,
}

impl StepProgress {
    pub fn new() -> Self {
        Self { completed: Vec::new(), running: None }
    }

    /// Start a new step. If there was a previous running step,
    /// it is automatically marked as completed first.
    pub fn start(&mut self, icon: &'static str, label: &str) {
        // Finalize any previous running step as auto-completed
        if self.running.is_some() {
            self.complete();
        }

        let step = RunningStep {
            icon,
            label: label.to_string(),
            start: Instant::now(),
            details: Vec::new(),
        };

        // Print the start of this step
        let elapsed = style("0.0s").dim();
        println!("  {}  {}  {}",
            style(icon).yellow(),
            style(label).white(),
            elapsed,
        );

        self.running = Some(step);
    }

    /// Mark the current step as completed with a checkmark.
    /// Rewrites the step's header line in-place.
    pub fn complete(&mut self) {
        let Some(step) = self.running.take() else { return };
        let elapsed = step.start.elapsed();
        let elapsed_str = format!("{:01}.{:01}s", elapsed.as_secs(), elapsed.subsec_millis() / 100);

        let detail_count = step.details.len();
        // Move cursor UP: 1 header + N detail lines
        let lines_up = 1 + detail_count;
        print!("\x1b[{}A\x1b[J", lines_up);
        // Print the completed header
        println!("  {}  {}  {}",
            style("✓").green(),
            style(&step.label).white(),
            style(elapsed_str).dim(),
        );
        // Re-print details
        for d in &step.details {
            println!("{}", d);
        }
        std::io::stdout().flush().ok();

        self.completed.push(step.label);
    }

    /// Mark the current step as failed.
    pub fn fail(&mut self, error: &str) {
        let Some(step) = self.running.take() else { return };
        let elapsed = step.start.elapsed();
        let elapsed_str = format!("{:01}.{:01}s", elapsed.as_secs(), elapsed.subsec_millis() / 100);

        let detail_count = step.details.len();
        let lines_up = 1 + detail_count;
        print!("\x1b[{}A\x1b[J", lines_up);
        println!("  {}  {}  {}",
            style("✗").red(),
            style(&step.label).red(),
            style(elapsed_str).dim(),
        );
        // Re-print details
        for d in &step.details {
            println!("{}", d);
        }
        if !error.is_empty() {
            println!("  {}  {}", style("└─").dim(), style(error).red().dim());
        }
        std::io::stdout().flush().ok();

        self.completed.push(step.label);
    }

    /// Add a detail line under the current running step.
    pub fn detail(&mut self, text: &str) {
        let Some(ref mut step) = self.running else { return };
        println!("{}", text);
        step.details.push(text.to_string());
        std::io::stdout().flush().ok();
    }

    /// Print a summary line after all steps are done.
    pub fn summary(&self, icon: &str, text: &str) {
        println!("  {}  {}", style(icon).dim(), style(text).dim());
    }

    /// Print plain text separator.
    pub fn separator() {
        let width = 60;
        println!("  {}", style("─".repeat(width)).dim());
    }

    /// Print token usage badge.
    pub fn token_badge(input: u32, output: u32) {
        println!("  {}  {}  {}  {}  {}  {}",
            style("■").dim(),
            style("↑").cyan(),
            style(input).cyan(),
            style("↓").yellow(),
            style(output).yellow(),
            style("tok").dim(),
        );
    }

    /// Print a text response (e.g., LLM output after all steps).
    pub fn response(text: &str) {
        for line in text.lines() {
            println!("  {}", line);
        }
    }

    /// Print the Rupoo identity header.
    pub fn banner() {
        println!("  {}  Rupoo — AI Terminal Assistant", style("⚡").cyan());
    }
}
